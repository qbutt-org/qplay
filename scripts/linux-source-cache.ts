import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { mkdir, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import { basename, dirname, join, resolve } from "node:path";
import { parseArgs } from "node:util";

export interface AptPackage {
    sourcePackage: string;
    sourceVersion: string;
}

export interface SourceArchive {
    url: string;
    sha256: string;
    size: number;
    file: string;
    name: string;
}

export interface AptSource {
    package: string;
    version: string;
    archives: SourceArchive[];
}

async function output(command: string[], cwd: string): Promise<string> {
    const child = Bun.spawn(command, { cwd, stdout: "pipe", stderr: "pipe" });
    const [text, error, status] = await Promise.all([
        new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited,
    ]);
    if (status !== 0)
        throw new Error(`${command.join(" ")} exited ${status}: ${error}\n${text.slice(-4000)}`);
    return text;
}

async function digest(path: string): Promise<string> {
    const hash = createHash("sha256");
    for await (const bytes of createReadStream(path))
        hash.update(bytes);
    return hash.digest("hex");
}

export async function collectAptSources(packages: Record<string, AptPackage>, cacheDirectory: string): Promise<AptSource[]> {
    const cache = resolve(cacheDirectory);
    await mkdir(cache, { recursive: true });
    const sources = new Map<string, AptPackage>();
    for (const item of Object.values(packages)) {
        if (!/^[a-z0-9][a-z0-9+.-]*$/.test(item.sourcePackage) || !/^[a-zA-Z0-9+.:~_-]+$/.test(item.sourceVersion))
            throw new Error("Invalid APT source package identity.");
        sources.set(`${item.sourcePackage}=${item.sourceVersion}`, item);
    }

    const index: AptSource[] = [];
    for (const [identity, item] of [...sources].sort(([left], [right]) => left.localeCompare(right))) {
        const metadata = await output(["apt-cache", "showsrc", "--only-source", item.sourcePackage], cache);
        const stanza = metadata.split(/\n\s*\n/).find(block =>
            block.split("\n").some(line => line === `Version: ${item.sourceVersion}`));
        if (!stanza)
            throw new Error(`The exact signed APT source record is unavailable: ${identity}`);
        const checksums = stanza.match(/^Checksums-Sha256:\n((?: .+\n?)+)/m)?.[1];
        if (!checksums)
            throw new Error(`The APT source has no SHA-256 inventory: ${identity}`);
        const expected = new Map<string, { size: number; sha256: string }>();
        for (const line of checksums.trimEnd().split("\n")) {
            const fields = line.trim().match(/^([a-f0-9]{64}) (\d+) (\S+)$/);
            if (!fields || basename(fields[3]) !== fields[3])
                throw new Error(`Invalid APT source archive metadata: ${identity}`);
            expected.set(fields[3], { sha256: fields[1], size: Number(fields[2]) });
        }

        // Ask APT to select the source origin. Security updates and their
        // original tarballs need not use the binary package's mirror.
        const uris = await output(["apt-get", "--print-uris", "--download-only", "--only-source", "source", identity], cache);
        const archives: SourceArchive[] = [];
        for (const line of uris.split("\n")) {
            const match = line.match(/^'([^']+)' (\S+) (\d+)(?: .+)?$/);
            if (!match)
                continue;
            const pin = expected.get(match[2]);
            if (!pin || Number(match[3]) !== pin.size || !/^https?:\/\//.test(match[1]))
                throw new Error(`APT source URI does not match its signed inventory: ${identity}`);
            const file = `${pin.sha256.slice(0, 16)}-${match[2]}`;
            const path = join(cache, file);
            if (!await Bun.file(path).exists()) {
                const partial = `${path}.partial`;
                try {
                    await output(["curl", "--fail", "--location", "--retry", "3", "--connect-timeout", "15", "--max-time", "600", "--output", partial, match[1]], cache);
                    if ((await stat(partial)).size !== pin.size || await digest(partial) !== pin.sha256)
                        throw new Error(`APT source download differs from its pin: ${match[2]}`);
                    await rename(partial, path);
                } finally {
                    await rm(partial, { force: true });
                }
            }
            if ((await stat(path)).size !== pin.size || await digest(path) !== pin.sha256)
                throw new Error(`Cached APT source differs from its pin: ${match[2]}`);
            archives.push({ url: match[1], ...pin, file, name: match[2] });
        }
        if (archives.length !== expected.size)
            throw new Error(`APT source archive inventory is incomplete: ${identity}`);
        index.push({ package: item.sourcePackage, version: item.sourceVersion, archives });
        console.log(`Verified source ${identity}: ${archives.length} archives`);
    }
    return index;
}

if (import.meta.main) {
    const { values } = parseArgs({ options: {
        packages: { type: "string" },
        cache: { type: "string" },
        output: { type: "string" },
    } });
    if (!values.packages || !values.cache || !values.output)
        throw new Error("Usage: bun scripts/linux-source-cache.ts --packages <sources.json> --cache <shared cache> --output <index.json>");
    const manifest = JSON.parse(await readFile(resolve(values.packages), "utf8"));
    const index = await collectAptSources(manifest.packages ?? manifest, values.cache);
    await mkdir(dirname(resolve(values.output)), { recursive: true });
    await writeFile(resolve(values.output), `${JSON.stringify(index, null, 4)}\n`);
}
