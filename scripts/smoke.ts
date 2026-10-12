import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { mkdir, mkdtemp, readFile, realpath, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join, relative, resolve } from "node:path";
import { parseArgs } from "node:util";

const { values } = parseArgs({ options: {
    bundle: { type: "string", default: join(tmpdir(), "qplay-build", "portable") },
    output: { type: "string" },
} });
const bundle = await realpath(resolve(values.bundle!));
const output = values.output ? resolve(values.output) : await mkdtemp(join(tmpdir(), "qplay-smoke-"));
await mkdir(output, { recursive: true });

function check(condition: unknown, message: string): asserts condition {
    if (!condition)
        throw new Error(message);
}

async function hash(path: string): Promise<string> {
    const digest = createHash("sha256");
    for await (const chunk of createReadStream(path))
        digest.update(chunk);
    return digest.digest("hex");
}

async function command(args: string[]): Promise<{ code: number; stdout: string; stderr: string }> {
    const process = Bun.spawn(args, { cwd: output, windowsHide: true, stdout: "pipe", stderr: "pipe" });
    const timeout = setTimeout(() => process.kill(), 30_000);
    try {
        const [stdout, stderr, code] = await Promise.all([
            new Response(process.stdout).text(), new Response(process.stderr).text(), process.exited,
        ]);
        return { code, stdout, stderr };
    }
    finally { clearTimeout(timeout); }
}

const manifest = JSON.parse(await readFile(join(bundle, "build-manifest.json"), "utf8"));
check(manifest.product === "qplay" && manifest.buildSystem === "CMake, CXX-Qt and Rust",
    "This is not a native qplay bundle.");
for (const [member, expected] of Object.entries<string>(manifest.files)) {
    const path = resolve(bundle, member);
    check(!relative(bundle, path).startsWith("..") && (await stat(path)).isFile(),
        `Missing bundle file: ${member}`);
    check(await hash(path) === expected, `Bundle file differs from its manifest: ${member}`);
}
check(manifest.files["qplay.exe"] && manifest.files["engine/libmpv-2.dll"], "Player or engine missing.");
check(!manifest.files["engine/profile/scripts/modernx.lua"], "Retired Lua UI was bundled.");

const player = join(bundle, "qplay.exe");
const version = await command([player, "--version-json"]);
check(version.code === 0 && JSON.parse(version.stdout).version === manifest.version,
    `Native version query failed: ${version.stderr}`);

const profile = join(output, "Профиль qplay");
const metadata = await command([player, "--runtime-info-json", `--profile-dir=${profile}`]);
check(metadata.code === 0, `Native runtime query failed: ${metadata.stderr}`);
const runtime = JSON.parse(metadata.stdout);
check(runtime.product === "qplay" && runtime.protocolVersion === 1, "Runtime protocol differs.");
check(resolve(runtime.configDir) === join(profile, "config") && resolve(runtime.dataDir) === join(profile, "data"),
    "Runtime query lost the Unicode profile path.");
for (const tool of ["mpv", "ffmpeg", "ffprobe", "ytDlp"] as const) {
    const path = runtime.tools[tool];
    check(isAbsolute(path) && !relative(join(bundle, "engine"), await realpath(path)).startsWith(".."),
        `Invalid ${tool} path.`);
    const versionOption = tool === "ffmpeg" || tool === "ffprobe" ? "-version" : "--version";
    const result = await command([path, versionOption]);
    check(result.code === 0, `${tool} failed from an unrelated working directory: ${result.stderr}`);
}

console.log(JSON.stringify({ bundle, nativeVersion: manifest.version, files: Object.keys(manifest.files).length,
    runtime: runtime.runtimeDir, profile }, null, 2));
