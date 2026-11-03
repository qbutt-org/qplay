import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { chmod, cp, mkdir, mkdtemp, readFile, readdir, realpath, rename, rm, stat, writeFile } from "node:fs/promises";
import { basename, join, resolve } from "node:path";
import { parseArgs } from "node:util";

if (process.platform !== "linux" || process.arch !== "x64")
    throw new Error("Build the Linux x64 runtime on its native platform.");

const root = resolve(import.meta.dir, "..");
const { values } = parseArgs({ options: {
    output: { type: "string" },
    "update-lock": { type: "boolean", default: false },
} });
if (!values.output)
    throw new Error("Usage: bun scripts/build-linux-runtime.ts --output <shared cache> [--update-lock]");
const output = resolve(values.output);
if (output === root || output.startsWith(`${root}/`))
    throw new Error("Keep runtime builds outside the checkout.");
const lockPath = join(root, "upstream-lock.json");
const lock = JSON.parse(await readFile(lockPath, "utf8"));
const pins = lock.linux;
const cache = join(output, "downloads");
const source = join(output, "source");
const prefix = join(output, "prefix");
await mkdir(cache, { recursive: true });
await mkdir(source, { recursive: true });

async function run(command: string[], cwd = output, environment: Record<string, string> = {}): Promise<string> {
    const child = Bun.spawn(command, { cwd, env: { ...process.env, ...environment }, stdout: "pipe", stderr: "inherit" });
    const text = await new Response(child.stdout).text();
    if (await child.exited !== 0)
        throw new Error(`Runtime command failed: ${command.join(" ")}\n${text.slice(-12_000)}`);
    return text.trim();
}

async function digest(path: string): Promise<string> {
    const hash = createHash("sha256");
    for await (const bytes of createReadStream(path))
        hash.update(bytes);
    return hash.digest("hex");
}

async function download(name: string, pin: { url: string; sha256: string; size: number }): Promise<string> {
    const path = join(cache, name);
    if (!await Bun.file(path).exists()) {
        const partial = `${path}.partial`;
        try {
            await run(["curl", "--fail", "--location", "--retry", "3", "--max-time", "600", "--output", partial, pin.url]);
            if (await digest(partial) !== pin.sha256 || (await stat(partial)).size !== pin.size)
                throw new Error(`Downloaded runtime source differs from its pin: ${name}`);
            await cp(partial, path);
        } finally {
            await rm(partial, { force: true });
        }
    }
    if (await digest(path) !== pin.sha256 || (await stat(path)).size !== pin.size)
        throw new Error(`Cached runtime source differs from its pin: ${name}`);
    return path;
}

const placebo = join(source, "libplacebo");
if (!await Bun.file(join(placebo, ".git/HEAD")).exists()) {
    await mkdir(placebo, { recursive: true });
    await run(["git", "init", placebo]);
    await run(["git", "remote", "add", "origin", "https://github.com/haasn/libplacebo.git"], placebo);
    await run(["git", "fetch", "--depth", "1", "origin", pins.libplacebo.revision], placebo);
    await run(["git", "checkout", "--detach", "FETCH_HEAD"], placebo);
}
if (await run(["git", "rev-parse", "HEAD"], placebo) !== pins.libplacebo.revision)
    throw new Error("The libplacebo source differs from its pin.");
await run(["git", "submodule", "update", "--init", "--recursive", "--depth", "1"], placebo);
if (await run(["git", "status", "--porcelain"], placebo))
    throw new Error("The libplacebo sources contain unrecorded changes.");
const submodules = await run(["git", "submodule", "status", "--recursive"], placebo);
if (submodules.split("\n").some(line => line.startsWith("+") || line.startsWith("-") || line.startsWith("U")))
    throw new Error("A libplacebo submodule differs from its source tree.");
const placeboBuild = join(output, "build-libplacebo");
if (!await Bun.file(join(placeboBuild, "build.ninja")).exists()) {
    await run(["meson", "setup", placeboBuild, placebo, `--prefix=${prefix}`, "--libdir=lib", "--buildtype=release",
        "-Ddemos=false", "-Dtests=false", "-Dbench=false", "-Dfuzz=false", "-Dvulkan=enabled", "-Dopengl=enabled"]);
}
await run(["meson", "compile", "-C", placeboBuild, "-j", "2"]);
await run(["meson", "install", "-C", placeboBuild]);

const ffmpegArchive = await download(`ffmpeg-${pins.ffmpeg.version}.tar.gz`, pins.ffmpeg);
const ffmpeg = join(source, `FFmpeg-${pins.ffmpeg.revision}`);
if (!await Bun.file(join(ffmpeg, "configure")).exists())
    await run(["tar", "-xzf", ffmpegArchive, "-C", source]);
const ffmpegBuild = join(output, `build-ffmpeg-${pins.ffmpeg.version}`);
await mkdir(ffmpegBuild, { recursive: true });
const buildEnvironment = { PKG_CONFIG_PATH: `${prefix}/lib/pkgconfig${process.env.PKG_CONFIG_PATH ? `:${process.env.PKG_CONFIG_PATH}` : ""}` };
const ffmpegOptions = [
    `--prefix=${prefix}`, "--enable-shared", "--disable-static", "--enable-pic", "--enable-gpl", "--enable-version3",
    "--enable-libx264", "--enable-libass", "--enable-gnutls", "--enable-libdav1d", "--enable-libopus",
    "--enable-libvpx", "--enable-libmp3lame", "--enable-vaapi", "--enable-vdpau", "--enable-nvdec", "--enable-nvenc",
    "--disable-ffplay", "--disable-doc", "--disable-debug",
];
const ffmpegConfiguration = join(ffmpegBuild, "ffbuild/config.mak");
const previousConfiguration = await Bun.file(ffmpegConfiguration).exists()
    ? (await readFile(ffmpegConfiguration, "utf8")).match(/^FFMPEG_CONFIGURATION=(.*)$/m)?.[1] : undefined;
if (previousConfiguration !== ffmpegOptions.join(" ")) {
    await run([join(ffmpeg, "configure"), ...ffmpegOptions], ffmpegBuild, buildEnvironment);
}
await run(["make", "-j2"], ffmpegBuild, buildEnvironment);
await run(["make", "install"], ffmpegBuild, buildEnvironment);

const mpvArchive = await download("mpv.tar.gz", pins.mpv);
const mpv = join(source, `mpv-${pins.mpv.revision}`);
if (!await Bun.file(join(mpv, "meson.build")).exists())
    await run(["tar", "-xzf", mpvArchive, "-C", source]);
const mpvBuild = join(output, "build-mpv");
const configuredMpv = await Bun.file(join(mpvBuild, "build.ninja")).exists();
let resetMpv = false;
if (configuredMpv) {
    const dependencies = JSON.parse(await run(["meson", "introspect", "--dependencies", mpvBuild])) as {
        name: string; version: string; compile_args: string[];
    }[];
    const codec = dependencies.find(dependency => dependency.name === "libavcodec");
    const version = await run(["pkg-config", "--modversion", "libavcodec"], output, buildEnvironment);
    resetMpv = !codec || codec.version !== version || !codec.compile_args.some(argument => argument.includes(prefix));
}
if (!configuredMpv || resetMpv) {
    await run(["meson", "setup", ...(resetMpv ? ["--wipe"] : []), mpvBuild, mpv, `--prefix=${prefix}`, "--libdir=lib", "--buildtype=release",
        "-Dlibmpv=true", "-Dcplayer=true", "-Dtests=false", "-Dvapoursynth=disabled", "-Dlua=lua5.2"], output, buildEnvironment);
}
await run(["meson", "compile", "-C", mpvBuild, "-j", "2"], output, buildEnvironment);
await run(["meson", "install", "-C", mpvBuild], output, buildEnvironment);

const stage = await mkdtemp(join(output, ".stage-"));
const engine = join(stage, "engine");
const tools = join(stage, "tools");
try {
    const aptPackages = new Map<string, {
        version: string;
        sourcePackage: string;
        sourceVersion: string;
        binary: { url: string; size: number; sha256: string };
    }>();
    const recordedLibraries = new Map<string, string>();
    const operatingSystemLibraries = /^(?:ld-linux-x86-64\.so\.2|lib(?:c|m|pthread|dl|rt|resolv|util|anl)\.so\.[0-9]+)$/;

    async function recordPackage(path: string): Promise<void> {
        if (path.startsWith(`${prefix}/`))
            return;
        const owner = (await run(["dpkg-query", "-S", path])).split("\n")[0].match(/^(.+?):\s/);
        if (!owner)
            throw new Error(`No package owns a bundled ELF dependency: ${path}`);
        const name = owner[1];
        if (aptPackages.has(name))
            return;
        const fields = (await run(["dpkg-query", "-W", "-f=${Version}\t${source:Package}\t${source:Version}", name])).split("\t");
        const uri = (await run(["apt-get", "--print-uris", "download", `${name}=${fields[0]}`]))
            .match(/^'([^']+)'\s+\S+\s+(\d+)\s+SHA\d+:[a-f0-9]+$/m);
        const sha256 = (await run(["apt-cache", "show", `${name}=${fields[0]}`]))
            .match(/^SHA256: ([a-f0-9]{64})$/m)?.[1];
        if (!uri || !sha256)
            throw new Error(`The exact package archive is unavailable: ${name}=${fields[0]}`);
        aptPackages.set(name, {
            version: fields[0], sourcePackage: fields[1], sourceVersion: fields[2],
            binary: { url: uri[1], size: Number(uri[2]), sha256 },
        });
        const copyright = `/usr/share/doc/${name.split(":")[0]}/copyright`;
        for (const directory of [tools, engine]) {
            const notices = join(directory, "licenses", name.replaceAll(":", "-"));
            await mkdir(notices, { recursive: true });
            await cp(copyright, join(notices, "copyright"));
        }
    }

    async function dependencies(input: string, libraryDirectory: string): Promise<void> {
        const listing = await run(["ldd", input], output, { LD_LIBRARY_PATH: `${prefix}/lib` });
        if (listing.includes("not found"))
            throw new Error(`Unresolved ELF dependency of ${input}`);
        for (const line of listing.split("\n")) {
            const match = line.match(/^\s*(\S+) => (\/\S+) \(/);
            if (!match || operatingSystemLibraries.test(match[1]))
                continue;
            const path = await realpath(match[2]);
            const identity = `${libraryDirectory}/${match[1]}`;
            const previous = recordedLibraries.get(identity);
            if (previous && previous !== path)
                throw new Error(`Conflicting ELF dependency: ${match[1]}`);
            if (previous)
                continue;
            recordedLibraries.set(identity, path);
            await mkdir(libraryDirectory, { recursive: true });
            const destination = join(libraryDirectory, match[1]);
            await cp(path, destination);
            await chmod(destination, 0o755);
            await run(["patchelf", "--set-rpath", "$ORIGIN", destination]);
            await recordPackage(path);
        }
    }

    await mkdir(engine, { recursive: true });
    await mkdir(tools, { recursive: true });
    for (const name of ["ffmpeg", "ffprobe"]) {
        const path = await realpath(join(prefix, "bin", name));
        for (const destination of [tools, engine]) {
            const executable = join(destination, name);
            await cp(path, executable);
            await chmod(executable, 0o755);
            await dependencies(path, join(destination, "lib"));
            await run(["patchelf", "--set-rpath", "$ORIGIN/lib", executable]);
        }
    }
    for (const [input, name] of [[join(prefix, "bin/mpv"), "mpv"], [join(prefix, "lib/libmpv.so.2"), "libmpv.so.2"]]) {
        const path = await realpath(input);
        const destination = join(engine, name);
        await cp(path, destination);
        await chmod(destination, 0o755);
        await dependencies(path, join(engine, "lib"));
        await run(["patchelf", "--set-rpath", "$ORIGIN/lib", destination]);
    }
    await recordPackage("/usr/include/ffnvcodec/nvEncodeAPI.h");
    const extractor = await download("yt-dlp_linux", pins.ytDlp);
    const denoArchive = await download(`deno-${pins.deno.version}-linux.zip`, pins.deno);
    const denoLicense = await download("deno-LICENSE.md", pins.deno.license);
    const denoNotices: { file: string; path: string }[] = [];
    for (const notice of pins.deno.notices) {
        if (basename(notice.file) !== notice.file)
            throw new Error("Invalid Deno notice filename.");
        denoNotices.push({ file: notice.file, path: await download(notice.file, notice) });
    }
    const deno = join(cache, "deno");
    await run(["unzip", "-o", denoArchive, "deno", "-d", cache]);
    if (await digest(deno) !== pins.deno.executableSha256)
        throw new Error("The Deno executable differs from its pin.");
    for (const directory of [tools, engine]) {
        await cp(extractor, join(directory, "yt-dlp"));
        await cp(deno, join(directory, "deno"));
        await chmod(join(directory, "yt-dlp"), 0o755);
        await chmod(join(directory, "deno"), 0o755);
        const notices = join(directory, "licenses");
        for (const name of ["yt-dlp-GPL-3.0.txt", "yt-dlp-THIRD-PARTY-LICENSES.txt", "yt-dlp-UNLICENSE.txt"])
            await cp(join(root, "licenses", name), join(notices, name));
        await cp(denoLicense, join(notices, "deno-LICENSE.md"));
        for (const notice of denoNotices)
            await cp(notice.path, join(notices, notice.file));
        await cp(join(mpv, "Copyright"), join(notices, "mpv-Copyright"));
        await cp(join(mpv, "LICENSE.GPL"), join(notices, "mpv-GPL"));
        await cp(join(placebo, "LICENSE"), join(notices, "libplacebo-LGPL"));
        await cp(join(ffmpeg, "COPYING.GPLv3"), join(notices, "ffmpeg-GPL-3.0"));
    }

    const sources = {
        platform: pins.platform,
        minimumGlibc: pins.minimumGlibc,
        mpv: pins.mpv,
        libplacebo: { ...pins.libplacebo, submodules: submodules.split("\n").map(line => line.trim()) },
        ffmpeg: pins.ffmpeg,
        ytDlp: pins.ytDlp,
        deno: pins.deno,
        frozenTools: pins.frozenTools,
        packages: Object.fromEntries([...aptPackages].sort(([left], [right]) => left.localeCompare(right))),
        ffmpegVersion: (await run([join(tools, "ffmpeg"), "-version"])).split("\n")[0],
    };
    for (const directory of [tools, engine])
        await writeFile(join(directory, "sources.json"), `${JSON.stringify(sources, null, 4)}\n`);

    async function inventory(directory: string, prefix = ""): Promise<Record<string, string>> {
        const files: Record<string, string> = {};
        for (const entry of await readdir(directory, { withFileTypes: true })) {
            const path = join(directory, entry.name);
            if (entry.isDirectory())
                Object.assign(files, await inventory(path, `${prefix}${entry.name}/`));
            else if (entry.isFile())
                files[`${prefix}${entry.name}`] = await digest(path);
            else
                throw new Error(`The runtime contains a link or special file: ${path}`);
        }
        return Object.fromEntries(Object.entries(files).sort(([left], [right]) => left.localeCompare(right)));
    }
    const newInventory = await inventory(engine);
    for (const name of ["engine", "tools"]) {
        const previous = join(output, name);
        if (await Bun.file(join(previous, "sources.json")).exists()) {
            const previousInventory = await inventory(previous);
            for (const [file, hash] of Object.entries(previousInventory)) {
                if (pins.runtime.files[file] !== hash)
                    throw new Error(`Refusing to replace an unowned or modified runtime file: ${name}/${file}`);
            }
            await rm(previous, { recursive: true });
        }
        await rename(join(stage, name), previous);
    }
    pins.runtime = { files: newInventory, sources };
    await writeFile(join(output, "linux-runtime.json"), `${JSON.stringify(pins, null, 2)}\n`);
    if (values["update-lock"])
        await writeFile(lockPath, `${JSON.stringify(lock, null, 2)}\n`);
    console.log(JSON.stringify({ engine: join(output, "engine"), tools: join(output, "tools"), lock: join(output, "linux-runtime.json"), engineSha256: pins.runtime.files["libmpv.so.2"] }, null, 4));
} finally {
    await rm(stage, { recursive: true, force: true });
}
