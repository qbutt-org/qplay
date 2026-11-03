import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { lstat, mkdir, mkdtemp, readFile, realpath, rename, rm, stat, symlink, writeFile } from "node:fs/promises";
import { basename, dirname, join, resolve } from "node:path";
import { parseArgs } from "node:util";
import type { AptSource } from "./linux-source-cache";

const { values } = parseArgs({ options: {
    runtime: { type: "string" },
    cache: { type: "string" },
    "qt-source": { type: "string" },
    output: { type: "string" },
} });
if (process.platform !== "linux" || !values.runtime || !values.cache || !values.output || !values["qt-source"])
    throw new Error("Usage on Linux: bun scripts/build-linux-sources.ts --runtime <runtime build> --cache <shared sources> --qt-source <Qt source archive> --output <sources.tar.gz>");
const root = resolve(import.meta.dir, "..");
const runtime = resolve(values.runtime);
const cache = resolve(values.cache);
const output = resolve(values.output);
if (output === root || output.startsWith(`${root}/`))
    throw new Error("Keep source archives outside the checkout.");
const lock = JSON.parse(await readFile(join(root, "upstream-lock.json"), "utf8"));
const apt: AptSource[] = JSON.parse(await readFile(join(cache, "apt-sources.json"), "utf8"));

async function command(argumentsList: string[], cwd = root): Promise<string> {
    const child = Bun.spawn(argumentsList, { cwd, stdout: "pipe", stderr: "inherit" });
    const result = await new Response(child.stdout).text();
    if (await child.exited !== 0)
        throw new Error(`Source archive command failed: ${argumentsList.join(" ")}`);
    return result.trimEnd();
}

async function digest(path: string): Promise<string> {
    const hash = createHash("sha256");
    for await (const bytes of createReadStream(path))
        hash.update(bytes);
    return hash.digest("hex");
}

await mkdir(dirname(output), { recursive: true });
const temporary = await mkdtemp(join(dirname(output), ".runtime-sources-"));
const staging = join(temporary, "sources");
await mkdir(staging);
const files: Record<string, { sha256: string; size: number }> = {};
try {
    async function add(relative: string, input: string, pin?: { sha256: string; size: number }): Promise<void> {
        if (relative.startsWith("/") || relative.split("/").some(part => !part || part === "." || part === "..") || files[relative])
            throw new Error(`Invalid or duplicate source member: ${relative}`);
        const metadata = await stat(input);
        if (!metadata.isFile())
            throw new Error(`The source member is not a regular file: ${input}`);
        const identity = { sha256: await digest(input), size: metadata.size };
        if (pin && (identity.size !== pin.size || identity.sha256 !== pin.sha256))
            throw new Error(`The source archive differs from its verified pin: ${input}`);
        const target = join(staging, relative);
        await mkdir(dirname(target), { recursive: true });
        await symlink(input, target);
        files[relative] = identity;
    }

    for (const source of apt) {
        for (const archive of source.archives) {
            if (basename(archive.file) !== archive.file || basename(archive.name) !== archive.name)
                throw new Error("Invalid APT source cache member.");
            await add(`apt/${source.package}/${source.version}/${archive.name}`, join(cache, "apt", archive.file), archive);
        }
    }
    for (const [name, cached] of [["ffmpeg", `ffmpeg-${lock.linux.ffmpeg.version}.tar.gz`], ["mpv", "mpv.tar.gz"]]) {
        const pin = lock.linux[name];
        await add(`native/${name}-${pin.revision}.tar.gz`, join(runtime, "downloads", cached), pin);
    }
    await add(`native/qt-everywhere-src-${lock.qt.version}.tar.xz`, resolve(values["qt-source"]), lock.qt.sourceArchive);
    for (const name of ["ytDlp", "deno"]) {
        const pin = lock.linux[name].sourceArchive;
        if (!pin?.revision || !pin?.sha256 || !pin?.size)
            throw new Error(`The exact ${name} source archive is not pinned.`);
        const file = `${name}-${pin.revision}.tar.gz`;
        const path = join(cache, "native", file);
        if (!await Bun.file(path).exists()) {
            await mkdir(dirname(path), { recursive: true });
            const partial = `${path}.partial`;
            try {
                await command(["curl", "--fail", "--location", "--retry", "3", "--retry-all-errors", "--connect-timeout", "15", "--max-time", "600", "--output", partial, pin.url]);
                if ((await stat(partial)).size !== pin.size || await digest(partial) !== pin.sha256)
                    throw new Error(`The downloaded source differs from its pin: ${name}`);
                await rename(partial, path);
            } finally {
                await rm(partial, { force: true });
            }
        }
        await add(`native/${file}`, path, pin);
    }

    const placebo = join(runtime, "source/libplacebo");
    if (await command(["git", "rev-parse", "HEAD"], placebo) !== lock.linux.libplacebo.revision)
        throw new Error("The libplacebo source revision differs from its pin.");
    if (await command(["git", "status", "--porcelain", "--untracked-files=no"], placebo))
        throw new Error("The pinned libplacebo source has tracked changes.");
    const submodules = await command(["git", "submodule", "status", "--recursive"], placebo);
    const sourceDirectories = [{ directory: placebo, prefix: "" }];
    for (const line of submodules.split("\n")) {
        const entry = line.match(/^ ([a-f0-9]{40}) ([^ ]+)(?: .*)?$/);
        if (!entry || entry[2].split("/").includes(".."))
            throw new Error("A pinned libplacebo submodule is missing or changed.");
        sourceDirectories.push({ directory: join(placebo, entry[2]), prefix: `${entry[2]}/` });
    }
    for (const { directory, prefix } of sourceDirectories) {
        if (await command(["git", "status", "--porcelain", "--untracked-files=no"], directory))
            throw new Error(`The libplacebo submodule has tracked changes: ${prefix}`);
        for (const name of (await command(["git", "ls-files", "-z"], directory)).split("\0").filter(Boolean)) {
            const input = join(directory, name);
            if ((await lstat(input)).isDirectory())
                continue; // The pinned submodule is added from its own tracked inventory.
            const actual = await realpath(input);
            if (!actual.startsWith(`${placebo}/`))
                throw new Error("A libplacebo source symlink escapes its source tree.");
            await add(`native/libplacebo/${prefix}${name}`, actual);
        }
    }
    for (const name of ["scripts/build-linux-runtime.ts", "scripts/linux-source-cache.ts", "scripts/build-linux-sources.ts", "upstream-lock.json"])
        await add(`build/${name}`, join(root, name));
    for (const name of await command(["git", "ls-files", "licenses"]).then(text => text.split("\n")))
        await add(`build/${name}`, join(root, name));
    await add("media-runtime.json", join(runtime, "engine/sources.json"));
    const sourceRevision = await command(["git", "rev-parse", "HEAD"]);
    await writeFile(join(staging, "source-index.json"), JSON.stringify({
        schemaVersion: 1, platform: "linux-x64", sourceRevision,
        runtime: {
            qt: lock.qt,
            mpv: lock.linux.mpv,
            libplacebo: { ...lock.linux.libplacebo, submodules: submodules.split("\n") },
            ffmpeg: lock.linux.ffmpeg,
            ytDlp: lock.linux.ytDlp,
            deno: lock.linux.deno,
        },
        apt, files,
    }, null, 4) + "\n");
    const archive = join(temporary, "archive.tar.gz");
    await command(["tar", "--dereference", "--sort=name", "--mtime=@0", "--owner=0", "--group=0", "--numeric-owner", "-czf", archive, "-C", staging, "."]);
    await rename(archive, output);
    const receipt = { archive: output, sha256: await digest(output), size: (await stat(output)).size, sourceRevision, aptSourcePackages: apt.length, files: Object.keys(files).length + 1 };
    await writeFile(`${output}.json`, JSON.stringify(receipt, null, 4) + "\n");
    console.log(JSON.stringify(receipt, null, 4));
} finally {
    await rm(temporary, { recursive: true, force: true });
}
