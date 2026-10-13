import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

const [build, runtime, mediaPath, qt, evidenceDirectory] = Bun.argv.slice(2).map(value => resolve(value));
assert(build && runtime && mediaPath && qt && evidenceDirectory,
    "Usage: bun tests/native/downloads.ts <build> <runtime> <legal media> <Qt SDK> <evidence>");
await mkdir(evidenceDirectory, { recursive: true });
const media = Bun.file(mediaPath);
let reads = 0;
const server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    fetch(request) {
        if (new URL(request.url).pathname !== "/legal.mp4")
            return new Response(null, { status: 404 });
        const range = request.headers.get("range");
        const match = range && /^bytes=(\d+)-(\d*)$/.exec(range);
        const start = match ? Number(match[1]) : 0;
        const end = match?.[2] ? Math.min(Number(match[2]), media.size - 1) : media.size - 1;
        if ((range && !match) || start > end || start < 0)
            return new Response(null, { status: 416 });
        ++reads;
        const headers = new Headers({ "Content-Type": "video/mp4", "Accept-Ranges": "bytes",
            "Content-Length": String(end - start + 1) });
        if (match)
            headers.set("Content-Range", `bytes ${start}-${end}/${media.size}`);
        return new Response(media.slice(start, end + 1), { status: match ? 206 : 200, headers });
    },
});
const executable = join(build, "qplay-native-acceptance.exe");
const prefix = join(evidenceDirectory, "downloads");
const profile = join(evidenceDirectory, "profile");
await mkdir(profile, { recursive: true });
const command = [join(runtime, "engine/libmpv-2.dll"), `http://127.0.0.1:${server.port}/legal.mp4`, mediaPath, profile, prefix];
let log = "";
let timedOut = false;
try {
    const child = spawn(executable, command, { windowsHide: true,
        env: { ...process.env, PATH: `${join(qt, "bin")};${process.env.PATH ?? ""}`,
            QT_PLUGIN_PATH: join(qt, "plugins"), QML_IMPORT_PATH: join(qt, "qml"),
            QT_COMMAND_LINE_PARSER_NO_GUI_MESSAGE_BOXES: "1", QPLAY_DOWNLOAD_QA: "1" },
        stdio: ["ignore", "pipe", "pipe"],
    });
    child.stdout.on("data", bytes => log += bytes.toString());
    child.stderr.on("data", bytes => log += bytes.toString());
    const timeout = setTimeout(() => { timedOut = true; child.kill(); }, process.env.QPLAY_DOWNLOAD_TIMEOUT_QA ? 150_000 : 60_000);
    const exitCode = await new Promise<number | null>((accept, reject) => {
        child.once("error", reject);
        child.once("close", accept);
    }).finally(() => clearTimeout(timeout));
    await writeFile(join(evidenceDirectory, "downloads.log"), log);
    assert(!timedOut && exitCode === 0, `Native Save fixture failed: ${exitCode}, timeout=${timedOut}\n${log.slice(-5000)}`);
    const evidence = JSON.parse(await readFile(`${prefix}.json`, "utf8"));
    assert.equal(evidence.exitCode, 0);
    for (const key of ["downloadExactRequest", "doubleClickSuppressed", "downloadFailureRetry",
        "staleTokenAndUrlIgnored", "localAndReplacementUnavailable", "receiverDeclarationRequired"])
        assert.equal(evidence[key], true, key);
    assert(reads > 0, "The fixture did not read legal media over HTTP");
    const sha256 = async (file: string) => createHash("sha256").update(await readFile(file)).digest("hex");
    const receipt = { command: [executable, ...command], evidence, reads,
        executableSha256: await sha256(executable), mediaSha256: await sha256(mediaPath) };
    await writeFile(join(evidenceDirectory, "receipt.json"), `${JSON.stringify(receipt, null, 4)}\n`);
    console.log(JSON.stringify(receipt, null, 4));
} finally {
    server.stop(true);
}
