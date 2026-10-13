import { mkdir, writeFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { join, resolve } from "node:path";

const [fixtureExecutable, evidenceDirectory, nativeExecutable, library, mediaFile, qtDirectory] = Bun.argv.slice(2);

if (!fixtureExecutable || !evidenceDirectory) {
    throw new Error("Usage: bun tests/native/subtitles-network.ts <fixture executable> <evidence directory>");
}
if ([nativeExecutable, library, mediaFile, qtDirectory].some(Boolean)
    && ![nativeExecutable, library, mediaFile, qtDirectory].every(Boolean)) {
    throw new Error("Native acceptance requires executable, mpv library, legal media and Qt SDK paths");
}
const profile = nativeExecutable ? join(resolve(evidenceDirectory), `native-profile-${randomUUID()}`) : undefined;
if (profile)
    await mkdir(profile, { recursive: true });

const srt = "1\n00:00:01,000 --> 00:00:03,000\nHello from the network\n\n";
const ass = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1280\nPlayResY: 720\n\n[V4+ Styles]\n"
    + "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n"
    + "Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,10,1\n\n[Events]\n"
    + "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
    + "Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,{\\an8}Hello from the network\n";
const nativeAss = ass.replace("0:00:01.00,0:00:03.00", "0:00:00.00,0:00:45.00")
    .replace("{\\an8}", "{\\an8}{\\pos(640,140)}");
const requests: { origin: string; method: string; path: string; authorization: boolean; cookie: boolean }[] = [];
let headerViolation = false;
let slowClosed = false;
let nativeSlowRequests = 0;
let nativeSlowClosed = false;

const foreign = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    fetch(request) {
        const authorization = request.headers.has("authorization");
        const cookie = request.headers.has("cookie");
        requests.push({ origin: "foreign", method: request.method, path: new URL(request.url).pathname, authorization, cookie });
        headerViolation ||= authorization || cookie;
        return new Response(srt);
    },
});

const primary = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    fetch(request) {
        const path = new URL(request.url).pathname;
        const authorization = request.headers.get("authorization") === "fixture-authorization";
        const cookie = request.headers.get("cookie") === "fixture-cookie";
        requests.push({ origin: "primary", method: request.method, path, authorization, cookie });
        if (!path.endsWith("native.mp4"))
            headerViolation ||= !authorization || !cookie;

        if (mediaFile && path.endsWith("native.mp4")) {
            const file = Bun.file(resolve(mediaFile));
            const headers = { "Content-Length": String(file.size), ETag: `"fixture-native-media-${file.size}"` };
            if (request.method === "HEAD")
                return new Response(null, { headers });
            const range = /^bytes=(\d+)-(\d*)$/.exec(request.headers.get("Range") ?? "");
            if (!range)
                return new Response(file, { headers });
            const start = Number(range[1]);
            const end = range[2] ? Number(range[2]) : file.size - 1;
            return new Response(file.slice(start, end + 1), { status: 206, headers: {
                ...headers, "Content-Length": String(end - start + 1), "Content-Range": `bytes ${start}-${end}/${file.size}`,
            } });
        }

        if (request.method === "HEAD") {
            const tag = path.endsWith("changed.mkv") ? '"changed-identity"' : '"stable-identity"';
            return new Response(null, { headers: {
                "Content-Length": path.endsWith("resized.mkv") ? "12346" : "12345",
                ETag: path.endsWith("weak.mkv") ? `W/${tag}` : tag,
            } });
        }

        if (path === "/redirect.ass")
            return Response.redirect(new URL("original.ass", primary.url));
        if (path === "/foreign.srt")
            return Response.redirect(new URL("original.srt", foreign.url));
        if (path === "/oversize.srt")
            return new Response(new Uint8Array(8 * 1024 * 1024 + 1).fill(65));
        if (path === "/streamed-oversize.srt") {
            let sent = 0;
            return new Response(new ReadableStream({
                pull(controller) {
                    if (sent++ === 9)
                        controller.close();
                    else
                        controller.enqueue(new Uint8Array(1024 * 1024).fill(65));
                },
            }));
        }
        if (path === "/slow.srt") {
            request.signal.addEventListener("abort", () => { slowClosed = true; }, { once: true });
            return new Response(new ReadableStream({
                cancel() { slowClosed = true; },
            }));
        }
        if (path === "/native.ass")
            return new Response(nativeAss);
        if (path === "/native-slow.ass") {
            if (++nativeSlowRequests === 1)
                return new Response(nativeAss);
            request.signal.addEventListener("abort", () => { nativeSlowClosed = true; }, { once: true });
            return new Response(new ReadableStream({
                start(controller) { controller.enqueue(new TextEncoder().encode(nativeAss)); },
                cancel() { nativeSlowClosed = true; },
            }));
        }
        return new Response(path.endsWith(".ass") ? ass : srt);
    },
});

try {
    const child = Bun.spawn([resolve(fixtureExecutable), primary.url.origin, foreign.url.origin, ...(profile ? [profile] : [])], {
        stdout: "pipe",
        stderr: "pipe",
        windowsHide: true,
    });
    const timeout = setTimeout(() => child.kill(), 30_000);
    const [stdout, stderr, exitCode] = await Promise.all([
        new Response(child.stdout).text(),
        new Response(child.stderr).text(),
        child.exited,
    ]).finally(() => clearTimeout(timeout));

    for (let attempt = 0; attempt < 20 && !slowClosed; ++attempt)
        await Bun.sleep(50);
    const evidence = { exitCode, headerViolation, slowClosed, requests, stdout, stderr };
    await mkdir(resolve(evidenceDirectory), { recursive: true });
    await writeFile(resolve(evidenceDirectory, "subtitle-network.json"), JSON.stringify(evidence, null, 4));

    if (exitCode !== 0 || headerViolation || !slowClosed
        || requests.some(request => request.path === "/header.srt")
        || requests.some(request => request.path === "/movie.mkv" || request.method === "GET" && request.path.includes("/media/") && !request.path.endsWith("native.mp4"))
        || requests.filter(request => request.path === "/media/0123456789abcdef0123456789abcdef/same.mkv").length !== 1
        || requests.filter(request => request.origin === "foreign").length !== 2) {
        throw new Error(`Subtitle network integration failed: ${JSON.stringify(evidence)}`);
    }
    console.log(stdout.trim());
    if (nativeExecutable && library && mediaFile && qtDirectory && profile) {
        const evidencePrefix = join(resolve(evidenceDirectory), "native-http");
        const child = Bun.spawn([resolve(nativeExecutable), resolve(library),
            new URL("media/0123456789abcdef0123456789abcdef/native.mp4", primary.url).href,
            resolve(mediaFile), profile, evidencePrefix], {
            stdout: "pipe", stderr: "pipe", windowsHide: true,
            env: {
                ...process.env,
                PATH: `${resolve(qtDirectory, "bin")};${process.env.PATH ?? ""}`,
                QT_PLUGIN_PATH: resolve(qtDirectory, "plugins"),
                QML_IMPORT_PATH: resolve(qtDirectory, "qml"),
                QPLAY_SUBTITLE_HTTP_QA: new URL("native.ass?fixture=signed", primary.url).href,
            },
        });
        const deadline = setTimeout(() => child.kill(), 60_000);
        const [stdout, stderr, exitCode] = await Promise.all([
            new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited,
        ]).finally(() => clearTimeout(deadline));
        await writeFile(join(resolve(evidenceDirectory), "native-http.log"), stdout + stderr);
        await writeFile(join(resolve(evidenceDirectory), "native-http-requests.json"), JSON.stringify({ nativeSlowRequests, nativeSlowClosed, requests }, null, 4));
        if (exitCode !== 0 || headerViolation || nativeSlowRequests !== 2 || !nativeSlowClosed)
            throw new Error(`Native HTTP subtitle acceptance exited ${exitCode}: ${stderr}`);
        const evidence = await Bun.file(`${evidencePrefix}.json`).json();
        if (!evidence.networkSubtitleCacheStyleOriginal || evidence.ownedChildren !== 0)
            throw new Error("Native HTTP subtitle acceptance did not confirm cache/style/Original/lifetime");
        console.log(`native-http PASS: ${evidencePrefix}.json`);
    }
} finally {
    primary.stop(true);
    foreign.stop(true);
}
