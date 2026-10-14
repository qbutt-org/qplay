import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { cp, mkdir, mkdtemp, readFile, readdir, realpath, rename, rm, rmdir, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative, resolve, sep } from "node:path";
import { parseArgs } from "node:util";

const root = resolve(import.meta.dir, "..");
const { values } = parseArgs({ options: {
    output: { type: "string", default: join(tmpdir(), "qplay-build") },
    "native-build": { type: "string" },
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
const nativeBuild = values["native-build"] ? resolve(values["native-build"]) : join(output, "native-build");
const cmakeCache = await readFile(join(nativeBuild, "CMakeCache.txt"), "utf8");
const configuredSource = cmakeCache.match(/^CMAKE_HOME_DIRECTORY:INTERNAL=(.+)$/m)?.[1];
const qtCmake = cmakeCache.match(/^Qt6_DIR:PATH=(.+)$/m)?.[1];
if (!configuredSource || !qtCmake)
    throw new Error("The native build is not configured with Qt 6.12.");
const qtBin = resolve(qtCmake, "../../..", "bin");
const windeployqt = join(qtBin, "windeployqt.exe");
if (!await Bun.file(windeployqt).exists())
    throw new Error(`Qt deployment tool is missing: ${windeployqt}`);
await mkdir(output, { recursive: true });
output = await realpath(output);

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

async function outputOf(command: string[], cwd = root): Promise<string> {
    const process = Bun.spawn(command, { cwd, windowsHide: true, stdout: "pipe", stderr: "inherit" });
    const output = await new Response(process.stdout).text();
    if (await process.exited !== 0)
        throw new Error(`Command failed: ${basename(command[0])}`);
    return output.trim();
}

const sourceRevision = await outputOf(["git", "rev-parse", "HEAD"]);
const nativeSourceRevision = await outputOf(["git", "rev-parse", "HEAD"], configuredSource);
const nativePaths = ["CMakeLists.txt", "src/native", "assets/icons", "assets/qplay.ico", "qplay-version.txt"];
for (const source of [root, configuredSource]) {
    const changes = await outputOf(["git", "status", "--porcelain", "--", ...nativePaths], source);
    if (changes)
        throw new Error(`Player sources contain uncommitted changes: ${source}`);
}
for (const path of nativePaths) {
    const packaged = await outputOf(["git", "rev-parse", `HEAD:${path}`]);
    const compiled = await outputOf(["git", "rev-parse", `HEAD:${path}`], configuredSource);
    if (packaged !== compiled)
        throw new Error(`The native build differs from packaged player source: ${path}`);
}
const qtVersion = await outputOf([join(qtBin, "qtpaths6.exe"), "--qt-version"]);
if (qtVersion !== lock.qt.version)
    throw new Error(`Qt ${qtVersion} differs from pinned ${lock.qt.version}.`);
const manager = lock.nativeProfile.framegenManager;
const managerSource = join(root, manager.path);
if (await hash(managerSource) !== manager.sha256)
    throw new Error("The frame generation manager differs from its native profile pin.");
const committedManager = await outputOf(["git", "rev-parse", `${manager.binaryRevision}:${manager.path}`]);
const bundledManager = await outputOf(["git", "hash-object", managerSource]);
if (committedManager !== bundledManager)
    throw new Error("The frame generation manager differs from its recorded binary revision.");
await run(["cmake", "--build", nativeBuild, "--config", "Release", "--target", "qplay-native", "all_qmllint"]);
const stage = await realpath(await mkdtemp(join(output, ".stage-")));

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
    const extractedProfile = await realpath(join(engine, "profile"));
    if (dirname(extractedProfile) !== await realpath(engine))
        throw new Error("The archive profile is outside the staged engine.");
    await rm(extractedProfile, { recursive: true });
    for (const item of [
        "certs/cacert.pem",
        "framegen",
        "shaders",
        "tools/bin/framegen-manager.exe",
        "tools/bin/framegen-manager-THIRD-PARTY-NOTICES.txt",
        "tools/bin/framegen-bootstrap.dll",
        "tools/bin/framegen-bootstrap-THIRD-PARTY-NOTICES.txt",
        "tools/bin/framegen-vsov.dll",
        "tools/bin/Rust-Standard-Library-COPYRIGHT.html",
    ]) {
        const destination = join(engine, "profile", item);
        await mkdir(dirname(destination), { recursive: true });
        await cp(join(root, "profile", item), destination, { recursive: true });
    }
    await cp(join(root, "licenses"), join(engine, "licenses"), { recursive: true });
    await cp(join(root, "LICENSE"), join(bundle, "LICENSE"));
    await cp(join(root, "upstream-lock.json"), join(bundle, "upstream-lock.json"));
    await cp(join(root, "assets/qplay.ico"), join(bundle, "qplay.ico"));
    const nativeExecutable = join(nativeBuild, "qplay-native.exe");
    if (!await Bun.file(nativeExecutable).exists())
        throw new Error(`The native player is missing: ${nativeExecutable}`);
    await cp(nativeExecutable, join(bundle, "qplay.exe"));
    const deployment = JSON.parse(await outputOf([windeployqt, "--release", "--json",
        "--qmldir", join(root, "src/native/qml"), "--translations", "en,ru",
        "--compiler-runtime", join(bundle, "qplay.exe")])) as {
        files: { source: string; target: string }[];
    };
    const redistributables = deployment.files.filter(file =>
        basename(file.source).toLowerCase() === "vc_redist.x64.exe");
    if (redistributables.length !== 1)
        throw new Error("Qt deployment did not locate one official x64 MSVC redistributable.");

    const redistributable = redistributables[0];
    const redistDirectory = dirname(await realpath(redistributable.source));
    const compiler = cmakeCache.match(/^CMAKE_CXX_COMPILER:FILEPATH=(.+)$/m)?.[1];
    if (!compiler)
        throw new Error("The native build does not identify its MSVC compiler.");
    const toolsetDirectory = resolve(dirname(await realpath(compiler)), "../../..");
    const toolset = basename(toolsetDirectory);
    const visualStudio = resolve(toolsetDirectory, "../../../..");
    const redistRoot = join(visualStudio, "VC/Redist/MSVC") + sep;
    if (!redistDirectory.toLowerCase().startsWith(redistRoot.toLowerCase()))
        throw new Error("The MSVC redistributable is outside the native compiler's Visual Studio installation.");
    const servicing = await readFile(join(redistDirectory, "Auxiliary/Microsoft.VC.ServicingVersion.Redist.props"), "utf8");
    const servicingVersion = servicing.match(/<VCServicingVersionRedist[^>]*>([0-9.]+)<[/]VCServicingVersionRedist>/)?.[1];
    if (servicingVersion !== toolset)
        throw new Error("The MSVC redistributable does not match the native compiler toolset.");

    const crtDirectory = join(redistDirectory, "x64/Microsoft.VC143.CRT");
    const crtFiles = await files(crtDirectory);
    if (crtFiles.some(file => basename(file) !== file || !file.endsWith(".dll"))
        || !["msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"].every(file => crtFiles.includes(file)))
        throw new Error("The official x64 MSVC CRT cohort is incomplete or invalid.");
    // Qt deploys an installer; portable playback needs its redistributable DLL cohort beside qplay.exe.
    for (const file of crtFiles)
        await cp(join(crtDirectory, file), join(bundle, file));
    if (await realpath(redistributable.target) !== await realpath(bundle))
        throw new Error("The MSVC redistributable installer is outside the staged bundle.");
    await rm(join(bundle, basename(redistributable.source)));
    const compilerRuntime = {
        toolset,
        redistributable: basename(redistDirectory),
        architecture: "x64",
        cohort: "Microsoft.VC143.CRT",
        installerSha256: await hash(redistributable.source),
        redistribution: "https://learn.microsoft.com/visualstudio/releases/2022/redistribution",
        license: "engine/licenses/MSVC-Runtime.rtf",
        files: crtFiles,
    };
    const inventory: Record<string, string> = {};
    for (const member of await files(bundle))
        inventory[member] = await hash(join(bundle, member));
    const sourceStatus = Bun.spawn(["git", "status", "--porcelain"], { cwd: root, stdout: "pipe", stderr: "inherit" });
    const sourceWorktreeChanged = Boolean((await new Response(sourceStatus.stdout).text()).trim());
    if (await sourceStatus.exited !== 0)
        throw new Error("Could not record source state.");
    const manifest = {
        product: "qplay", version, protocolVersion: 1, platform: "windows-x64",
        sourceRevision,
        nativeSourceRevision,
        sourceWorktreeChanged,
        buildSystem: "CMake, CXX-Qt and Rust",
        qtVersion,
        compilerRuntime,
        runtimeArchive: { url: lock.runtime.url, sha256: lock.runtime.sha256, size: lock.runtime.size },
        framegenManager: { sha256: manager.sha256, binaryRevision: manager.binaryRevision },
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
    await run(["tar.exe", "-a", "-cf", portableArchive, "-C", bundle, "."]);
    const publishedBundle = join(output, "portable");
    if (await Bun.file(join(publishedBundle, "build-manifest.json")).exists()) {
        const old = JSON.parse(await readFile(join(publishedBundle, "build-manifest.json"), "utf8"));
        if (old.product !== "qplay")
            throw new Error("Refusing to replace an unowned portable output.");
        if (dirname(await realpath(publishedBundle)) !== output)
            throw new Error("Portable output is outside the build directory.");
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
