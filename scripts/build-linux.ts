import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { cp, mkdir, mkdtemp, readFile, readdir, realpath, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative, resolve } from "node:path";
import { parseArgs } from "node:util";

const root = resolve(import.meta.dir, "..");
const { values } = parseArgs({ options: {
    output: { type: "string", default: join(tmpdir(), "qplay-build") },
    "native-build": { type: "string" },
    "linux-runtime": { type: "string" },
} });
if (process.platform !== "linux" || process.arch !== "x64" || !values["native-build"] || !values["linux-runtime"])
    throw new Error("Usage on Linux x64: bun run build -- --native-build <build> --linux-runtime <verified engine> [--output <directory>]");
const output = resolve(values.output!);
const nativeBuild = resolve(values["native-build"]);
const runtime = resolve(values["linux-runtime"]);
if (output === root || output.startsWith(`${root}/`))
    throw new Error("Build output must be outside the source repository.");
const lock = JSON.parse(await readFile(join(root, "upstream-lock.json"), "utf8"));
const version = (await readFile(join(root, "qplay-version.txt"), "utf8")).trim();
const cmakeCache = await readFile(join(nativeBuild, "CMakeCache.txt"), "utf8");
const configuredSource = cmakeCache.match(/^CMAKE_HOME_DIRECTORY:INTERNAL=(.+)$/m)?.[1];
const qtCmake = cmakeCache.match(/^Qt6_DIR:PATH=(.+)$/m)?.[1];
if (!configuredSource || !qtCmake || !/^\d+\.\d+\.\d+$/.test(version))
    throw new Error("The native build or product version is invalid.");

async function run(command: string[], cwd = root): Promise<string> {
    const child = Bun.spawn(command, { cwd, stdout: "pipe", stderr: "inherit" });
    const text = await new Response(child.stdout).text();
    if (await child.exited !== 0)
        throw new Error(`Build command failed: ${command.join(" ")}\n${text.slice(-12_000)}`);
    return text.trim();
}

async function digest(path: string): Promise<string> {
    const hash = createHash("sha256");
    for await (const chunk of createReadStream(path))
        hash.update(chunk);
    return hash.digest("hex");
}

async function files(directory: string, prefix = ""): Promise<string[]> {
    const names: string[] = [];
    for (const entry of await readdir(directory, { withFileTypes: true })) {
        if (entry.isDirectory())
            names.push(...await files(join(directory, entry.name), `${prefix}${entry.name}/`));
        else if (entry.isFile() || entry.isSymbolicLink())
            names.push(`${prefix}${entry.name}`);
        else
            throw new Error(`Invalid build file: ${entry.name}`);
    }
    return names.sort();
}

const sourceRevision = await run(["git", "rev-parse", "HEAD"]);
const nativeSourceRevision = await run(["git", "rev-parse", "HEAD"], configuredSource);
for (const directory of [root, configuredSource]) {
    if (await run(["git", "status", "--porcelain", "--", "CMakeLists.txt", "src/native", "assets", "qplay-version.txt", "upstream-lock.json"], directory))
        throw new Error(`The native source has uncommitted changes: ${directory}`);
}
for (const path of ["CMakeLists.txt", "src/native", "assets", "qplay-version.txt", "upstream-lock.json"]) {
    if (await run(["git", "rev-parse", `HEAD:${path}`]) !== await run(["git", "rev-parse", `HEAD:${path}`], configuredSource))
        throw new Error(`Compiled and packaged sources differ: ${path}`);
}
const qtPrefix = resolve(qtCmake, "../../..");
const qtVersion = await run([join(qtPrefix, "bin/qtpaths"), "--qt-version"]);
if (qtVersion !== lock.qt.version)
    throw new Error(`Qt ${qtVersion} differs from pinned ${lock.qt.version}.`);
const runtimeFiles = await files(runtime);
if (JSON.stringify(runtimeFiles) !== JSON.stringify(Object.keys(lock.linux.runtime.files).sort()))
    throw new Error("The Linux runtime file inventory differs from its pin.");
for (const name of runtimeFiles) {
    if (await digest(join(runtime, name)) !== lock.linux.runtime.files[name])
        throw new Error(`The Linux runtime differs from its pin: ${name}`);
}
await run(["cmake", "--build", nativeBuild, "--target", "qplay-native", "all_qmllint", "--parallel", "2"]);
await mkdir(output, { recursive: true });
const stage = await mkdtemp(join(output, ".stage-"));
const bundle = join(stage, "portable");
try {
    await run(["cmake", "--install", nativeBuild, "--prefix", bundle]);
    await cp(runtime, join(bundle, "engine"), { recursive: true });
    for (const name of ["certs", "shaders"])
        await cp(join(root, "profile", name), join(bundle, "engine/profile", name), { recursive: true });
    for (const name of ["certifi-MPL-2.0.txt", "Qt-LGPL-3.0.txt"])
        await cp(join(root, "licenses", name), join(bundle, "engine/licenses", name));
    for (const name of ["LICENSE", "upstream-lock.json"])
        await cp(join(root, name), join(bundle, name));
    await cp(join(root, "assets/qplay.svg"), join(bundle, "qplay.svg"));

    // Qt's deployment uses SONAME links. Release verification and the native
    // installer deliberately own regular files with one immutable inventory.
    for (const name of await files(bundle)) {
        const path = join(bundle, name);
        const resolved = await realpath(path);
        if (resolved !== path) {
            const bytes = await readFile(resolved);
            await rm(path);
            await writeFile(path, bytes, { mode: 0o755 });
        }
    }
    const inventory: Record<string, string> = {};
    for (const name of await files(bundle)) {
        const path = join(bundle, name);
        const header = await Bun.file(path).slice(0, 4).arrayBuffer();
        if (Buffer.from(header).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46])) && !name.startsWith("engine/")) {
            const library = relative(dirname(path), join(bundle, "lib")) || ".";
            await run(["patchelf", "--set-rpath", `$ORIGIN/${library}`, path]);
        }
        inventory[name] = await digest(path);
    }
    for (const name of Object.keys(inventory)) {
        if (name.startsWith("translations/") && name !== "translations/qt_en.qm" && name !== "translations/qt_ru.qm") {
            await rm(join(bundle, name));
            delete inventory[name];
            continue;
        }
        if (!name.startsWith("lib/") || !basename(name).includes(".so"))
            continue;
        const soname = await run(["patchelf", "--print-soname", join(bundle, name)]);
        const runtimeName = `lib/${soname}`;
        if (soname && !soname.includes("/") && runtimeName !== name && inventory[runtimeName] === inventory[name]) {
            await rm(join(bundle, name));
            delete inventory[name];
        }
    }
    const manifest = {
        product: "qplay", version, protocolVersion: 1, platform: "linux-x64",
        sourceRevision, nativeSourceRevision,
        sourceWorktreeChanged: Boolean(await run(["git", "status", "--porcelain"])),
        buildSystem: "CMake, CXX-Qt and Rust", qtVersion,
        minimumGlibc: lock.linux.minimumGlibc,
        runtime: lock.linux.runtime.sources,
        files: inventory,
    };
    await writeFile(join(bundle, "build-manifest.json"), `${JSON.stringify(manifest, null, 4)}\n`);
    // Installed Linux is the default; --install copies only manifest-owned
    // files, so the portable profile marker never enters an installation.
    await writeFile(join(bundle, "qplay.portable"), "qplay portable profile: userdata/config and userdata/data\n");
    const archive = join(output, "qplay-linux-x64.zip");
    await rm(archive, { force: true });
    await run(["zip", "-q", "-r", archive, "."], bundle);
    const publishedBundle = join(output, "portable");
    if (await Bun.file(join(publishedBundle, "build-manifest.json")).exists()) {
        const old = JSON.parse(await readFile(join(publishedBundle, "build-manifest.json"), "utf8"));
        if (old.product !== "qplay")
            throw new Error("The output directory belongs to another product.");
        await rm(publishedBundle, { recursive: true });
    }
    await rename(bundle, publishedBundle);
    console.log(JSON.stringify({ archive, sha256: await digest(archive), bundle: publishedBundle, files: Object.keys(inventory).length }, null, 4));
} finally {
    await rm(stage, { recursive: true, force: true });
}
