import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { chmod, copyFile, mkdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { createConnection, type Socket } from "node:net";
import { join, resolve } from "node:path";

const [bundleArgument, mediaArgument, evidenceArgument, archiveArgument] = Bun.argv.slice(2);
if (process.platform !== "linux" || !bundleArgument || !mediaArgument || !evidenceArgument || !archiveArgument)
    throw new Error("Usage on Linux: bun tests/native/linux.ts <portable bundle> <legal media> <evidence directory> <portable ZIP>");
const bundle = resolve(bundleArgument);
const media = resolve(mediaArgument);
const archive = resolve(archiveArgument);
const evidence = resolve(evidenceArgument);
const repository = resolve(import.meta.dir, "../..");
assert(evidence !== repository && !evidence.startsWith(`${repository}/`), "Keep evidence outside the checkout.");
const home = join(evidence, "home");
const data = join(home, "data");
const config = join(home, "config");
const cache = join(home, "cache");
const runtime = join(home, "run");
for (const directory of [home, data, config, cache, runtime])
    await mkdir(directory, { recursive: true, mode: 0o700 });
await chmod(runtime, 0o700);
const environment: Record<string, string | undefined> = {
    ...process.env,
    HOME: home,
    XDG_DATA_HOME: data,
    XDG_CONFIG_HOME: config,
    XDG_CACHE_HOME: cache,
    XDG_RUNTIME_DIR: runtime,
    PATH: "/usr/bin:/bin",
};
delete environment.QT_PLUGIN_PATH;
delete environment.QML_IMPORT_PATH;
delete environment.QML2_IMPORT_PATH;
delete environment.LD_LIBRARY_PATH;
const portable = join(bundle, "qplay");
const installed = join(data, "qplay/app/qplay");
const log: string[] = [];

async function run(program: string, argumentsList: string[], accepted = 0, expectedError?: string): Promise<string> {
    const child = Bun.spawn([program, ...argumentsList], {
        cwd: "/", env: environment, stdout: "pipe", stderr: "pipe",
    });
    const timeout = setTimeout(() => child.kill(), 30_000);
    const [output, error, status] = await Promise.all([
        new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited,
    ]).finally(() => clearTimeout(timeout));
    log.push(JSON.stringify({ program, argumentsList, status, output, error }));
    assert.equal(status, accepted, `${program}: ${error}`);
    if (expectedError)
        assert(error.includes(expectedError), `${program}: expected ${expectedError}, got ${error}`);
    return output.trim();
}

const version = JSON.parse(await run(portable, ["--version-json"]));
assert.equal(version.protocolVersion, 1);
assert.equal(version.product, "qplay");
await mkdir(join(config, "qplay"), { recursive: true });
await writeFile(join(config, "qplay/preferences.json"), JSON.stringify({
    language: "en", appearance: "light", audio_delay: 0,
    shader: "off", framegen: "off", content: "auto",
}));
const sentinel = join(data, "qplay/preserved.fixture");
await mkdir(join(data, "qplay"), { recursive: true });
await writeFile(sentinel, "User playback data must survive installation and removal.\n");
assert.equal(await run(portable, ["--install"]), installed);
assert(!await Bun.file(join(data, "qplay/app/qplay.portable")).exists());
assert.equal(await run(portable, ["--install"]), installed);
const metadata = JSON.parse(await run(installed, ["--runtime-info-json"]));
assert.equal(metadata.protocolVersion, 1);
assert.equal(metadata.configDir, join(config, "qplay"));
assert.equal(metadata.dataDir, join(data, "qplay"));
assert.equal(metadata.cacheDir, join(cache, "qplay"));
for (const [name, path] of Object.entries(metadata.tools) as [string, string][]) {
    assert(path.startsWith(`${data}/qplay/app/engine/`), `Tool ownership: ${name}`);
    await run(path, [name === "mpv" || name === "ytDlp" ? "--version" : "-version"]);
}
assert(metadata.onlineTools.deno?.path);
await run(metadata.onlineTools.deno.path, ["--version"]);
const inspected = JSON.parse(await run(metadata.tools.ffprobe, [
    "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,sample_aspect_ratio", "-of", "json", media,
]));
const expectedVideo = inspected.streams[0];
assert(expectedVideo?.width > 0 && expectedVideo?.height > 0);

const socketPath = join(runtime, "qbutt-mpv-linux.sock");
assert(Buffer.byteLength(socketPath) < 108, "Use a shorter evidence directory for Unix IPC.");
const player = spawn(installed, [
    `--input-ipc-server=${socketPath}`,
    `--script-opts-append=ytdl_hook-ytdl_path=${metadata.tools.ytDlp}`,
    "--resume-playback=no", media,
], { cwd: "/", env: environment, stdio: ["ignore", "pipe", "pipe"] });
let playerLog = "";
player.stdout.on("data", bytes => playerLog += bytes.toString());
player.stderr.on("data", bytes => playerLog += bytes.toString());
const exited = new Promise<number | null>((accept, reject) => {
    player.once("error", reject);
    player.once("close", accept);
});
const playerTimeout = setTimeout(() => player.kill("SIGKILL"), 45_000);
let socket: Socket | undefined;
try {
    for (let attempt = 0; attempt < 100; ++attempt) {
        try {
            socket = await new Promise<Socket>((accept, reject) => {
                const connection = createConnection(socketPath);
                connection.once("error", reject);
                connection.once("connect", () => accept(connection));
            });
            break;
        } catch {
            await Bun.sleep(100);
        }
    }
    assert(socket, `The native IPC socket did not become available. ${playerLog}`);
    let requestId = 0;
    let pendingBytes = "";
    const pending = new Map<number, { accept: (value: unknown) => void; reject: (error: Error) => void }>();
    socket.on("data", bytes => {
        pendingBytes += bytes.toString();
        for (let newline; (newline = pendingBytes.indexOf("\n")) !== -1;) {
            const message = JSON.parse(pendingBytes.slice(0, newline));
            pendingBytes = pendingBytes.slice(newline + 1);
            const request = pending.get(message.request_id);
            if (!request)
                continue;
            pending.delete(message.request_id);
            if (message.error === "success")
                request.accept(message.data);
            else
                request.reject(new Error(`mpv IPC: ${message.error}`));
        }
    });
    const command = (argumentsList: unknown[]) => new Promise<unknown>((accept, reject) => {
        const id = ++requestId;
        const timeout = setTimeout(() => { pending.delete(id); reject(new Error("Native IPC request timed out.")); }, 5000);
        pending.set(id, {
            accept: value => { clearTimeout(timeout); accept(value); },
            reject: error => { clearTimeout(timeout); reject(error); },
        });
        socket!.write(`${JSON.stringify({ command: argumentsList, request_id: id })}\n`);
    });
    let duration = 0;
    for (let attempt = 0; attempt < 100 && !duration; ++attempt) {
        try {
            duration = Number(await command(["get_property", "duration"]));
        } catch {
            await Bun.sleep(100);
        }
    }
    assert(duration >= 8, "Use a legal fixture at least eight seconds long.");
    assert.equal(await command(["get_property", "path"]), media);
    const width = Number(await command(["get_property", "video-params/w"]));
    const height = Number(await command(["get_property", "video-params/h"]));
    assert.equal(width, expectedVideo.width);
    assert.equal(height, expectedVideo.height);
    const outputWidth = Number(await command(["get_property", "video-out-params/w"]));
    const outputHeight = Number(await command(["get_property", "video-out-params/h"]));
    assert.equal(outputWidth, expectedVideo.width);
    assert.equal(outputHeight, expectedVideo.height);
    const [sarNumerator, sarDenominator] = expectedVideo.sample_aspect_ratio.split(":").map(Number);
    const pixelAspectRatio = Number(await command(["get_property", "video-out-params/par"]));
    assert(Math.abs(pixelAspectRatio - sarNumerator / sarDenominator) < 0.00001);
    await command(["set_property", "pause", true]);
    assert.equal(await command(["get_property", "pause"]), true);
    await command(["seek", 4, "absolute+exact"]);
    for (let attempt = 0; attempt < 100; ++attempt) {
        if (Math.abs(Number(await command(["get_property", "time-pos"])) - 4) < 0.5)
            break;
        await Bun.sleep(50);
    }
    assert(Math.abs(Number(await command(["get_property", "time-pos"])) - 4) < 0.5);
    await command(["set_property", "pause", false]);
    await Bun.sleep(500);
    assert(Number(await command(["get_property", "time-pos"])) > 4);
    await run(portable, ["--install"], 1, "Another qplay window");
    await run(installed, [`--install-from=${archive}`], 1, "Another qplay window");
    await command(["quit"]);
    assert.equal(await exited, 0);
    await writeFile(join(evidence, "playback.json"), `${JSON.stringify({ duration, width, height, outputWidth, outputHeight, pixelAspectRatio, pauseSeekResume: true, concurrentInstallRefused: true }, null, 4)}\n`);
} finally {
    clearTimeout(playerTimeout);
    socket?.destroy();
    if (player.exitCode === null) {
        player.kill("SIGKILL");
        await exited;
    }
    await writeFile(join(evidence, "player.log"), playerLog);
}

const beforeUpdate = await stat(installed);
assert.equal(await run(installed, [`--install-from=${archive}`]), installed);
assert.notEqual((await stat(installed)).ino, beforeUpdate.ino, "The archive update did not replace the installation.");
assert(!await Bun.file(join(data, "qplay/app/qplay.portable")).exists());
const malformed = join(evidence, "invalid.zip");
await writeFile(malformed, "Generated invalid ZIP fixture.\n");
const beforeFailure = await stat(installed);
await run(installed, [`--install-from=${malformed}`], 1, "Read qplay update");
assert.equal((await stat(installed)).ino, beforeFailure.ino, "A failed archive update changed the installed application.");
await rm(malformed);

// A raw central-directory duplicate remains visible in EOCD counts even though
// zip 4.6 collapses it in its filename map. Reuse the real release payload.
const archiveBytes = await readFile(archive);
const footerOffset = archiveBytes.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
assert(footerOffset > 0);
const footer = Buffer.from(archiveBytes.subarray(footerOffset));
const directoryOffset = footer.readUInt32LE(16);
let directoryEntry = directoryOffset;
let duplicate: Buffer | undefined;
while (directoryEntry < footerOffset) {
    assert.equal(archiveBytes.readUInt32LE(directoryEntry), 0x02014b50);
    const nameLength = archiveBytes.readUInt16LE(directoryEntry + 28);
    const entryLength = 46 + nameLength + archiveBytes.readUInt16LE(directoryEntry + 30) + archiveBytes.readUInt16LE(directoryEntry + 32);
    if (archiveBytes.subarray(directoryEntry + 46, directoryEntry + 46 + nameLength).toString() === "build-manifest.json")
        duplicate = archiveBytes.subarray(directoryEntry, directoryEntry + entryLength);
    directoryEntry += entryLength;
}
assert(duplicate);
const duplicateFooter = Buffer.from(footer);
duplicateFooter.writeUInt16LE(footer.readUInt16LE(8) + 1, 8);
duplicateFooter.writeUInt16LE(footer.readUInt16LE(10) + 1, 10);
duplicateFooter.writeUInt32LE(footer.readUInt32LE(12) + duplicate.length, 12);
const duplicateArchive = join(evidence, "duplicate.zip");
await writeFile(duplicateArchive, Buffer.concat([archiveBytes.subarray(0, footerOffset), duplicate, duplicateFooter]));
await run(installed, [`--install-from=${duplicateArchive}`], 1, "duplicate file names");
assert.equal((await stat(installed)).ino, beforeFailure.ino);
await rm(duplicateArchive);

// ZIP64 can carry unsaturated legacy fields. Its explicit locator must still
// be refused by the bounded ZIP32 release format.
const zip64 = Buffer.alloc(76);
zip64.writeUInt32LE(0x06064b50, 0);
zip64.writeBigUInt64LE(44n, 4);
zip64.writeUInt16LE(45, 12);
zip64.writeUInt16LE(45, 14);
zip64.writeBigUInt64LE(BigInt(footer.readUInt16LE(10)), 24);
zip64.writeBigUInt64LE(BigInt(footer.readUInt16LE(10)), 32);
zip64.writeBigUInt64LE(BigInt(footer.readUInt32LE(12)), 40);
zip64.writeBigUInt64LE(BigInt(directoryOffset), 48);
zip64.writeUInt32LE(0x07064b50, 56);
zip64.writeBigUInt64LE(BigInt(footerOffset), 64);
zip64.writeUInt32LE(1, 72);
const zip64Archive = join(evidence, "zip64.zip");
await writeFile(zip64Archive, Buffer.concat([archiveBytes.subarray(0, footerOffset), zip64, footer]));
await run(installed, [`--install-from=${zip64Archive}`], 1, "single-disk ZIP32");
assert.equal((await stat(installed)).ino, beforeFailure.ino);
await rm(zip64Archive);

const unlistedArchive = join(evidence, "unlisted.zip");
const unlistedMember = join(evidence, "unlisted.fixture");
await copyFile(archive, unlistedArchive);
await writeFile(unlistedMember, "Generated unlisted archive member.\n");
const append = Bun.spawn(["/usr/bin/zip", "-q", unlistedArchive, "unlisted.fixture"], { cwd: evidence });
assert.equal(await append.exited, 0);
await run(installed, [`--install-from=${unlistedArchive}`], 1, "unowned file");
assert.equal((await stat(installed)).ino, beforeFailure.ino);
assert(!await Bun.file(join(data, "qplay/app/unlisted.fixture")).exists());
await rm(unlistedArchive);
await rm(unlistedMember);
const unsafeDirectory = join(evidence, "unsafe-archive");
const unsafeArchive = join(evidence, "unsafe.zip");
await mkdir(unsafeDirectory);
await writeFile(join(evidence, "escape.fixture"), "Generated unsafe ZIP member.\n");
await writeFile(join(unsafeDirectory, "build-manifest.json"), JSON.stringify({
    product: "qplay", version: version.version, protocolVersion: 1, files: {},
}));
const zip = Bun.spawn(["/usr/bin/zip", "-q", unsafeArchive, "../escape.fixture", "build-manifest.json"], { cwd: unsafeDirectory });
assert.equal(await zip.exited, 0);
await run(installed, [`--install-from=${unsafeArchive}`], 1, "unsafe path");
assert.equal((await stat(installed)).ino, beforeFailure.ino);
assert(!await Bun.file(join(data, "qplay/escape.fixture")).exists());
await rm(unsafeArchive);
await rm(join(evidence, "escape.fixture"));
await rm(unsafeDirectory, { recursive: true });
const afterUpdate = JSON.parse(await run(installed, ["--runtime-info-json"]));
assert.equal(afterUpdate.version, version.version);

const unowned = join(data, "qplay/app/preserved-unowned.fixture");
await writeFile(unowned, "Unrelated files may not be deleted.\n");
await run(installed, ["--uninstall"], 1, "unowned file");
assert(await Bun.file(unowned).exists());
await rm(unowned);
await run(installed, ["--uninstall"]);
assert(!await Bun.file(installed).exists());
assert(await Bun.file(sentinel).exists());
assert(await Bun.file(join(config, "qplay/preferences.json")).exists());
const sha256 = (path: string) => readFile(path).then(bytes => createHash("sha256").update(bytes).digest("hex"));
const receipt = {
    version, metadata,
    portableSha256: await sha256(portable), mediaSha256: await sha256(media),
    installRepeatUninstall: true, dataPreserved: true, unownedFilesPreserved: true,
    archiveUpdate: true, malformedUpdatePreservedInstallation: true,
    unsafeArchivePreservedInstallation: true,
    duplicateArchivePreservedInstallation: true,
    zip64ArchivePreservedInstallation: true,
    unlistedArchivePreservedInstallation: true,
    toolsWorkedWithoutSdkOrCallerPath: true,
};
await writeFile(join(evidence, "commands.jsonl"), `${log.join("\n")}\n`);
await writeFile(join(evidence, "receipt.json"), `${JSON.stringify(receipt, null, 4)}\n`);
console.log(JSON.stringify(receipt, null, 4));
