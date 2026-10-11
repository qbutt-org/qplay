import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { cp, mkdir, mkdtemp, readFile, readdir, realpath, rename, rm, rmdir, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative, resolve } from "node:path";
import { parseArgs } from "node:util";

const root = resolve(import.meta.dir, "..");
const { values } = parseArgs({ options: {
    output: { type: "string", default: join(tmpdir(), "qplay-build") },
    "runtime-archive": { type: "string" },
    "installer-compiler": { type: "string" },
} });
let output = resolve(values.output!);
if (output === root || output.startsWith(`${root}\\`) || output.startsWith(`${root}/`))
    throw new Error("Build output must be outside the source repository.");
const version = (await readFile(join(root, "qplay-version.txt"), "utf8")).trim();
if (!/^\d+\.\d+\.\d+$/.test(version))
    throw new Error("Invalid qplay version.");
const lock = JSON.parse(await readFile(join(root, "upstream-lock.json"), "utf8"));
await mkdir(output, { recursive: true });
output = await realpath(output);
const stage = await realpath(await mkdtemp(join(output, ".stage-")));

async function hash(path: string): Promise<string> {
    const digest = createHash("sha256");
    for await (const chunk of createReadStream(path))
        digest.update(chunk);
    return digest.digest("hex");
}

async function run(command: string[], cwd = root): Promise<void> {
    const process = Bun.spawn(command, { cwd, windowsHide: true, stdout: "inherit", stderr: "inherit" });
    if (await process.exited !== 0)
        throw new Error(`Build command failed: ${basename(command[0])}`);
}

async function verifiedDownload(path: string, pin: { url: string; sha256: string; size?: number }): Promise<void> {
    if (!await Bun.file(path).exists()) {
        await mkdir(dirname(path), { recursive: true });
        const partial = `${path}.partial`;
        try {
            await run(["curl.exe", "--fail", "--location", "--retry", "3", "--max-time", "600", "--output", partial, pin.url]);
            if (await hash(partial) !== pin.sha256 || (pin.size !== undefined && Bun.file(partial).size !== pin.size))
                throw new Error(`Downloaded dependency failed its pinned integrity check: ${basename(path)}`);
            await cp(partial, path);
        }
        finally { await rm(partial, { force: true }); }
    }
    if (await hash(path) !== pin.sha256 || (pin.size !== undefined && Bun.file(path).size !== pin.size))
        throw new Error(`Dependency differs from its exact pin: ${basename(path)}`);
}

async function files(directory: string, prefix = ""): Promise<string[]> {
    const result: string[] = [];
    for (const entry of await readdir(directory, { withFileTypes: true })) {
        const name = prefix + entry.name;
        if (entry.isSymbolicLink())
            throw new Error(`Release input must not be a symbolic link: ${name}`);
        if (entry.isDirectory())
            result.push(...await files(join(directory, entry.name), `${name}/`));
        else if (entry.isFile())
            result.push(name);
    }
    return result.sort();
}

try {
    const bundle = join(stage, "portable");
    const engine = join(bundle, "engine");
    await mkdir(engine, { recursive: true });
    const archive = values["runtime-archive"] ? resolve(values["runtime-archive"]) : join(output, "cache", basename(new URL(lock.runtime.url).pathname));
    await verifiedDownload(archive, lock.runtime);
    const listing = Bun.spawn(["tar.exe", "-tf", archive], { stdout: "pipe", stderr: "inherit" });
    const members = (await new Response(listing.stdout).text()).trimEnd().split(/\r?\n/).sort();
    if (await listing.exited !== 0 || JSON.stringify(members) !== JSON.stringify(Object.keys(lock.runtime.files).sort()))
        throw new Error("The engine archive differs from its pinned file inventory.");
    for (const member of members)
        if (!member || member.includes("\\") || member.includes(":") || member.startsWith("/")
            || member.split("/").some(part => !part || part === "." || part === ".."))
            throw new Error("Unsafe engine archive member.");
    await run(["tar.exe", "-xf", archive, "-C", engine]);
    for (const [member, expected] of Object.entries(lock.runtime.files))
        if (await hash(join(engine, member)) !== expected)
            throw new Error(`Pinned engine member differs: ${member}`);
    // Native helpers share the engine DLL closure. Keep each executable beside
    // those DLLs so an absolute tool path works from any caller's directory.
    await rename(join(engine, "tools", "ffmpeg.exe"), join(engine, "ffmpeg.exe"));
    await rename(join(engine, "profile", "tools", "bin", "ffprobe.exe"), join(engine, "ffprobe.exe"));
    await rename(join(engine, "tools", "yt-dlp.exe"), join(engine, "yt-dlp.exe"));
    await rmdir(join(engine, "tools"));
    await cp(join(root, "profile"), join(engine, "profile"), { recursive: true,
        filter: path => !path.split(/[\\/]/).includes("target") });
    await cp(join(root, "licenses"), join(engine, "licenses"), { recursive: true });
    await cp(join(root, "LICENSE"), join(bundle, "LICENSE"));
    await cp(join(root, "upstream-lock.json"), join(bundle, "upstream-lock.json"));
    await cp(join(root, "assets/qplay.ico"), join(bundle, "qplay.ico"));
    await writeFile(join(stage, "AssemblyInfo.cs"), [
        "using System.Reflection;",
        `[assembly: AssemblyVersion("${version}.0")]`,
        `[assembly: AssemblyFileVersion("${version}.0")]`,
        "[assembly: AssemblyProduct(\"qplay\")]",
        "[assembly: AssemblyCompany(\"qbutt-org\")]",
        "[assembly: AssemblyDescription(\"qplay media player\")]",
        ""
    ].join("\n"));
    const compiler = join(process.env.SystemRoot || "C:/Windows", "Microsoft.NET/Framework64/v4.0.30319/csc.exe");
    await run([compiler, "/nologo", "/target:winexe", "/platform:x64", "/optimize+", "/debug-",
        "/reference:System.Windows.Forms.dll", "/reference:System.Drawing.dll", "/reference:System.Web.Extensions.dll",
        "/reference:System.Net.Http.dll", `/win32icon:${join(root, "assets/qplay.ico")}`,
        `/win32manifest:${join(root, "src/app.manifest")}`, `/out:${join(bundle, "qplay.exe")}`,
        ...((await files(join(root, "src"))).filter(file => file.endsWith(".cs")).map(file => join(root, "src", file))),
        join(stage, "AssemblyInfo.cs")]);
    await writeFile(join(bundle, "qplay.exe.config"), '<?xml version="1.0"?><configuration><startup><supportedRuntime version="v4.0" sku=".NETFramework,Version=v4.8" /></startup></configuration>\n');
    const inventory: Record<string, string> = {};
    for (const member of await files(bundle))
        inventory[member] = await hash(join(bundle, member));
    const revision = Bun.spawn(["git", "rev-parse", "HEAD"], { cwd: root, stdout: "pipe", stderr: "ignore" });
    const sourceRevision = (await new Response(revision.stdout).text()).trim();
    const sourceStatus = Bun.spawn(["git", "status", "--porcelain"], { cwd: root, stdout: "pipe", stderr: "inherit" });
    const sourceWorktreeChanged = Boolean((await new Response(sourceStatus.stdout).text()).trim());
    if (await sourceStatus.exited !== 0)
        throw new Error("Could not record source state.");
    const manifest = {
        product: "qplay", version, protocolVersion: 1, platform: "windows-x64",
        sourceRevision: await revision.exited === 0 ? sourceRevision : null,
        sourceWorktreeChanged,
        compiler: "Microsoft .NET Framework v4 C# compiler",
        runtimeArchive: { url: lock.runtime.url, sha256: lock.runtime.sha256, size: lock.runtime.size },
        correspondingSources: lock.sourceArchive,
        files: inventory
    };
    await writeFile(join(bundle, "build-manifest.json"), JSON.stringify(manifest, null, 4) + "\n");
    const installerCache = join(process.env.LOCALAPPDATA!, "qplay", "build", "dependencies");
    const installerTools = join(installerCache, `inno-setup-${lock.buildTools.innoSetup.version}`);
    const installerCompiler = values["installer-compiler"] || join(installerTools, "ISCC.exe");
    if (!values["installer-compiler"] && !await Bun.file(installerCompiler).exists()) {
        const installer = join(installerCache, `innosetup-${lock.buildTools.innoSetup.version}.exe`);
        await verifiedDownload(installer, lock.buildTools.innoSetup);
        await run([installer, "/CURRENTUSER", "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", "/NOICONS", "/TASKS=",
            `/DIR=${installerTools}`]);
    }
    await run([installerCompiler, "/Qp", `/DAppVersion=${version}`, `/DBundleDir=${bundle}`, `/DProjectDir=${root}`,
        `/O${output}`, join(root, "dist/windows/qplay.iss")]);
    await writeFile(join(bundle, "qplay.portable"), "qplay portable profile: userdata/config and userdata/data\n");
    const portableArchive = join(output, "qplay-portable.zip");
    await run(["tar.exe", "-a", "-cf", portableArchive, "-C", bundle, ...await files(bundle)]);
    const publishedBundle = join(output, "portable");
    if (await Bun.file(join(publishedBundle, "build-manifest.json")).exists()) {
        const old = JSON.parse(await readFile(join(publishedBundle, "build-manifest.json"), "utf8"));
        if (old.product !== "qplay")
            throw new Error("Refusing to replace an unowned portable output.");
        await rm(publishedBundle, { recursive: true });
    }
    await cp(bundle, publishedBundle, { recursive: true });
    const artifacts = [];
    for (const name of ["qplay-setup.exe", "qplay-portable.zip"]) {
        const path = join(output, name);
        artifacts.push({ name, size: (await stat(path)).size, sha256: await hash(path) });
    }
    await writeFile(join(output, "build-manifest.json"), JSON.stringify(manifest, null, 4) + "\n");
    await writeFile(join(output, "release-manifest.json"), JSON.stringify({
        product: "qplay", version, protocolVersion: 1, artifacts
    }, null, 4) + "\n");
    console.log(JSON.stringify({ output, bundle: publishedBundle, artifacts }, null, 4));
}
finally { await rm(stage, { recursive: true, force: true }); }
