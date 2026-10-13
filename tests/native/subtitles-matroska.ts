import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { randomUUID } from "node:crypto";

const [fixtureExecutable, ffmpegFile, mkvmergeFile, srtFile, vttFile, evidenceFile, qtDirectory,
    nativeExecutable, runtimeLibrary, nativeFile, evidenceDirectory, cancelExecutable] = Bun.argv.slice(2);
if (!fixtureExecutable || !ffmpegFile || !mkvmergeFile || !srtFile || !vttFile || !evidenceFile || !qtDirectory) {
    throw new Error("Usage: bun tests/native/subtitles-matroska.ts <fixture.exe> <ffmpeg.mkv> <mkvmerge.mkv> <srt.mkv> <vtt.mkv> <evidence.json> <Qt SDK> [native.exe runtime-library native.mkv evidence-directory cancel.exe]");
}
if ([nativeExecutable, runtimeLibrary, nativeFile, evidenceDirectory].some(Boolean)
    && ![nativeExecutable, runtimeLibrary, nativeFile, evidenceDirectory].every(Boolean)) {
    throw new Error("Native acceptance requires its executable, library, media, Qt SDK and evidence directory");
}
const fixtureEnvironment = {
    ...process.env,
    Path: `${resolve(qtDirectory, "bin")};${process.env.Path ?? process.env.PATH ?? ""}`,
};

const fixtureFiles = [
    { name: "ffmpeg.mkv", path: ffmpegFile, track: 2, codec: "ass" },
    { name: "mkvmerge.mkv", path: mkvmergeFile, track: 2, codec: "ass" },
    { name: "srt.mkv", path: srtFile, track: 2, codec: "subrip" },
    { name: "vtt.mkv", path: vttFile, track: 3, codec: "webvtt" },
    ...(nativeFile ? [{ name: "native.mkv", path: nativeFile, track: 3, codec: "ass" }] : []),
];
const media = new Map(await Promise.all(fixtureFiles.map(async item => [item.name, await readFile(resolve(item.path))] as const)));
const requests: { name: string; query: string; method: string; range: string | null; bytes: number; authorization: boolean; cookie: boolean; ifMatch: boolean }[] = [];
let violation = false;
let slowClosed = false;
const foreignRequests: { method: string; authorization: boolean; cookie: boolean }[] = [];

const foreign = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    fetch(request) {
        const authorization = request.headers.has("authorization");
        const cookie = request.headers.has("cookie");
        foreignRequests.push({ method: request.method, authorization, cookie });
        violation ||= authorization || cookie;
        const data = media.get("ffmpeg.mkv")!;
        const etag = '"foreign-representation"';
        if (request.method === "HEAD")
            return new Response(null, { headers: { ETag: etag, "Content-Length": String(data.byteLength) } });
        const range = /^bytes=(\d+)-(\d+)$/.exec(request.headers.get("range") ?? "");
        if (!range || request.headers.get("if-match") !== etag)
            return new Response(null, { status: 412 });
        const start = Number(range[1]);
        const end = Number(range[2]);
        const body = data.subarray(start, end + 1);
        return new Response(body, { status: 206, headers: {
            ETag: etag, "Content-Length": String(body.byteLength),
            "Content-Range": `bytes ${start}-${end}/${data.byteLength}`,
        } });
    },
});

const server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    async fetch(request) {
        const url = new URL(request.url);
        const name = url.pathname.split("/").at(-1)!;
        const query = url.search;
        if (name === "redirect.mkv")
            return Response.redirect(new URL("foreign.mkv", foreign.url));
        const data = media.get(name === "slow.mkv" ? "ffmpeg.mkv" : name);
        if (!data)
            return new Response(null, { status: 404 });
        const authorization = request.headers.get("authorization") === "fixture-authorization";
        const cookie = request.headers.get("cookie") === "fixture-cookie";
        violation ||= !authorization || !cookie;
        const etag = `"fixture-${name}-${data.byteLength}"`;
        const headers: Record<string, string> = { ETag: etag, "Accept-Ranges": "bytes", "Content-Length": String(data.byteLength) };
        const range = request.headers.get("range");
        if (request.method === "HEAD") {
            requests.push({ name, query, method: "HEAD", range, bytes: 0, authorization, cookie, ifMatch: false });
            return new Response(null, { headers });
        }
        const ifMatch = request.headers.get("if-match");
        if (!authorization || !cookie) {
            requests.push({ name, query, method: "GET", range, bytes: 0, authorization, cookie, ifMatch: !!ifMatch });
            return new Response(null, { status: 401 });
        }
        if (ifMatch && ifMatch !== etag)
            return new Response(null, { status: 412 });
        if (!range) {
            requests.push({ name, query, method: "GET", range, bytes: data.byteLength, authorization, cookie, ifMatch: !!ifMatch });
            return new Response(data, { headers });
        }
        const match = /^bytes=(\d+)-(\d*)$/.exec(range);
        if (!match)
            return new Response(null, { status: 416 });
        const start = Number(match[1]);
        const end = match[2] ? Math.min(Number(match[2]), data.byteLength - 1) : data.byteLength - 1;
        if (start < 0 || start >= data.byteLength || start > end)
            return new Response(null, { status: 416 });
        const body = data.subarray(start, end + 1);
        if (name === "slow.mkv") {
            requests.push({ name, query, method: "GET", range, bytes: 0, authorization, cookie, ifMatch: !!ifMatch });
            await new Promise<void>(resolve => {
                const timeout = setTimeout(resolve, 5_000);
                request.signal.addEventListener("abort", () => {
                    slowClosed = true;
                    clearTimeout(timeout);
                    resolve();
                }, { once: true });
            });
            if (slowClosed)
                return new Response(null, { status: 499 });
        }
        requests.push({ name, query, method: "GET", range, bytes: body.byteLength, authorization, cookie, ifMatch: !!ifMatch });
        return new Response(body, { status: 206, headers: {
            ...headers,
            "Content-Length": String(body.byteLength),
            "Content-Range": `bytes ${start}-${end}/${data.byteLength}`,
        } });
    },
});

const cases = [];
let nativeEvidence;
try {
    for (const item of fixtureFiles.filter(item => item.name !== "native.mkv")) {
        const name = item.name;
        const url = new URL(`media/0123456789abcdef0123456789abcdef/${name}`, server.url).href;
        const child = Bun.spawn([resolve(fixtureExecutable), url, String(item.track), "Second cue", item.codec], {
            stdout: "pipe", stderr: "pipe", windowsHide: true,
            env: fixtureEnvironment,
        });
        const timeout = setTimeout(() => child.kill(), 20_000);
        const [stdout, stderr, exitCode] = await Promise.all([
            new Response(child.stdout).text(),
            new Response(child.stderr).text(),
            child.exited,
        ]).finally(() => clearTimeout(timeout));
        cases.push({ name, exitCode, stdout: stdout.trim(), stderr: stderr.trim() });
    }
    const redirect = Bun.spawn([resolve(fixtureExecutable), new URL("redirect.mkv?fixture=signed", server.url).href,
        "2", "Second cue", "ass"], {
        stdout: "pipe", stderr: "pipe", windowsHide: true,
        env: fixtureEnvironment,
    });
    const redirectResult = await Promise.all([
        new Response(redirect.stdout).text(), new Response(redirect.stderr).text(), redirect.exited,
    ]);
    cases.push({ name: "cross-origin-redirect", exitCode: redirectResult[2], stdout: redirectResult[0].trim(), stderr: redirectResult[1].trim() });

    if (cancelExecutable) {
        const cancel = Bun.spawn([resolve(cancelExecutable), new URL("media/0123456789abcdef0123456789abcdef/slow.mkv", server.url).href], {
            stdout: "pipe", stderr: "pipe", windowsHide: true,
            env: fixtureEnvironment,
        });
        const cancelResult = await Promise.all([
            new Response(cancel.stdout).text(), new Response(cancel.stderr).text(), cancel.exited,
        ]);
        for (let attempt = 0; attempt < 20 && !slowClosed; ++attempt)
            await Bun.sleep(50);
        cases.push({ name: "pending-cancel", exitCode: cancelResult[2], stdout: cancelResult[0].trim(), stderr: cancelResult[1].trim() });
    }
    if (nativeExecutable && runtimeLibrary && nativeFile && evidenceDirectory) {
        const profile = join(resolve(evidenceDirectory), `embedded-profile-${randomUUID()}`);
        const prefix = join(resolve(evidenceDirectory), "embedded-player");
        await mkdir(profile, { recursive: true });
        const url = new URL("media/0123456789abcdef0123456789abcdef/native.mkv", server.url).href;
        const seed = Bun.spawn([resolve(fixtureExecutable), url, "3", "Second cue", "ass", "12000", "19000", profile], {
            stdout: "pipe", stderr: "pipe", windowsHide: true,
            env: fixtureEnvironment,
        });
        const seedOutput = await Promise.all([new Response(seed.stdout).text(), new Response(seed.stderr).text(), seed.exited]);
        if (seedOutput[2] !== 0)
            throw new Error(`Embedded cache seed failed: ${seedOutput[1]}`);
        const native = Bun.spawn([resolve(nativeExecutable), resolve(runtimeLibrary), url, resolve(nativeFile), profile, prefix], {
            stdout: "pipe", stderr: "pipe", windowsHide: true,
            env: {
                ...fixtureEnvironment,
                QT_PLUGIN_PATH: resolve(qtDirectory, "plugins"),
                QML_IMPORT_PATH: resolve(qtDirectory, "qml"),
                QT_COMMAND_LINE_PARSER_NO_GUI_MESSAGE_BOXES: "1",
                QPLAY_SUBTITLE_MKV_QA: "1",
            },
        });
        const timeout = setTimeout(() => native.kill(), 60_000);
        const [stdout, stderr, exitCode] = await Promise.all([
            new Response(native.stdout).text(), new Response(native.stderr).text(), native.exited,
        ]).finally(() => clearTimeout(timeout));
        nativeEvidence = { exitCode, stdout, stderr, profile, prefix };
        await writeFile(`${prefix}.log`, stdout + stderr);
        if (exitCode !== 0)
            throw new Error(`Embedded native acceptance failed: ${stderr}`);
        nativeEvidence = { ...nativeEvidence, result: await Bun.file(`${prefix}.json`).json() };
    }
} finally {
    server.stop(true);
    foreign.stop(true);
}

const evidence = { cases, requests, foreignRequests, slowClosed,
    totalBytes: requests.reduce((sum, request) => sum + request.bytes, 0), violation, nativeEvidence };
await writeFile(resolve(evidenceFile), JSON.stringify(evidence, null, 2));
if (violation || cases.some(item => item.exitCode !== 0)
    || foreignRequests.length !== 2 || cancelExecutable && !slowClosed)
    throw new Error(`Indexed Matroska integration failed: ${JSON.stringify(evidence)}`);
console.log(JSON.stringify(evidence));
