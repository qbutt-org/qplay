import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { mkdir, readFile, readdir, unlink, writeFile } from "node:fs/promises";
import { basename, join, resolve } from "node:path";

const [fixtureArgument, ffmpegArgument, ffprobeArgument, evidenceArgument, nativeExecutable, mpvLibrary] = Bun.argv.slice(2);
assert(fixtureArgument && ffmpegArgument && ffprobeArgument && evidenceArgument,
    "Usage: bun tests/native/content-probe.ts <content-probe executable> <ffmpeg> <ffprobe> <new evidence directory> [native acceptance executable mpv library]");
assert(Boolean(nativeExecutable) === Boolean(mpvLibrary), "Native policy acceptance needs both executable and library");
const fixture = resolve(fixtureArgument);
const ffmpeg = resolve(ffmpegArgument);
const ffprobe = resolve(ffprobeArgument);
const evidence = resolve(evidenceArgument);
assert(!evidence.startsWith(resolve(import.meta.dir, "../..") + "/"));
await mkdir(evidence);
const anime = join(evidence, "anime 'quoted'; a,b.mkv");
const movie = join(evidence, "movie.mkv");
const hls = join(evidence, "hls");
const requests: { path: string; method: string; roleProbe: boolean; ranged: boolean; bytes: number; userAgent: string }[] = [];
const results: Record<string, unknown> = {};
let protectedRequests = 0;
let credentialsObserved = false;
let slowClosed = false;
let nativeSlowStarted = 0;
let nativeSlowClosed = false;
const activeStreams = new Set<ReadableStreamDefaultController>();
const environment: Record<string, string | undefined> = { ...process.env, HOME: join(evidence, "home"),
    LIBGL_ALWAYS_SOFTWARE: "1", GALLIUM_DRIVER: "llvmpipe", LP_NUM_THREADS: "1" };
for (const key of ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy", "OPENROUTER_API_KEY"])
    delete environment[key];
await mkdir(environment.HOME!);

async function run(program: string, args: string[], timeoutMs = 95000) {
    const child = Bun.spawn([program, ...args], { cwd: evidence, env: environment, stdout: "pipe", stderr: "pipe" });
    const timer = setTimeout(() => child.kill("SIGKILL"), timeoutMs);
    const [stdout, stderr, exit] = await Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited])
        .finally(() => clearTimeout(timer));
    assert.equal(exit, 0, `${basename(program)} exited ${exit}: ${stderr}`);
    assert(!stdout.includes("fixture-private-query") && !stderr.includes("fixture-private-query"), "Probe output exposed a private URL query");
    return { stdout, stderr };
}
async function classify(name: string, source: string, mode: string, expectedKind?: string, selection?: string) {
    const args = [ffprobe, source, "16", mode, ...(selection ? [selection] : [])];
    const output = await run(fixture, args);
    const result = JSON.parse(output.stdout);
    if (expectedKind) {
        assert.equal(result.kind, expectedKind, name);
        assert(result.confidence >= 0.55, name);
        assert(Math.abs(result.fps - 24) < 0.001 && !result.variable, `${name}: timing changed`);
    }
    results[name] = result;
    return result;
}

await run(ffmpeg, ["-v", "error", "-nostdin", "-f", "lavfi", "-i",
    "testsrc2=size=640x360:rate=6,drawgrid=w=32:h=32:t=4:c=white,fps=24",
    "-t", "16", "-an", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "0", "-g", "24", "-threads", "1", anime]);
await run(ffmpeg, ["-v", "error", "-nostdin", "-f", "lavfi", "-i",
    "color=c=gray:s=640x360:r=24,noise=alls=30:allf=t+u,hue=s=0",
    "-t", "16", "-an", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "0", "-g", "24", "-threads", "1", movie]);
await mkdir(hls);
await run(ffmpeg, ["-v", "error", "-nostdin", "-i", anime, "-c", "copy", "-hls_time", "2", "-hls_list_size", "0",
    "-hls_segment_filename", join(hls, "anime-%02d.ts"), "-f", "hls", join(hls, "anime.m3u8")]);
for (const file of [anime, movie]) {
    const decoded = await run(ffmpeg, ["-v", "error", "-nostdin", "-threads", "1", "-i", file, "-f", "null", "-"]);
    assert.equal(decoded.stderr, "");
}
const served = new Map([["/anime.mkv", anime], ["/movie.mkv", movie], ["/media/0123456789abcdef0123456789abcdef/file", anime]]);
const cancelledRangePath = "/media/fedcba9876543210fedcba9876543210/file";
served.set(cancelledRangePath, anime);
for (const name of await readdir(hls)) served.set(`/hls/${name}`, join(hls, name));
const server = Bun.serve({ hostname: "127.0.0.1", port: 0, fetch(request) {
    const url = new URL(request.url);
    credentialsObserved ||= request.headers.has("authorization") || request.headers.has("cookie");
    if (url.pathname === "/protected") {
        ++protectedRequests;
        return new Response(null, { status: 401 });
    }
    if (url.pathname === "/slow") {
        const closed = () => { slowClosed = true; };
        request.signal.addEventListener("abort", closed, { once: true });
        return new Response(new ReadableStream({
            start(controller) { activeStreams.add(controller); },
            cancel() { closed(); },
        }), { headers: { "Content-Type": "video/x-matroska", "Content-Length": String(Bun.file(anime).size) } });
    }
    if (url.pathname === cancelledRangePath && url.searchParams.get("role") === "probe") {
        ++nativeSlowStarted;
        const closed = () => { nativeSlowClosed = true; };
        request.signal.addEventListener("abort", closed, { once: true });
        return new Response(new ReadableStream({
            start(controller) { activeStreams.add(controller); },
            cancel() { closed(); },
        }), { headers: { "Content-Type": "video/x-matroska", "Content-Length": String(Bun.file(anime).size) } });
    }
    const path = served.get(url.pathname);
    if (!path) return new Response(null, { status: 404 });
    const file = Bun.file(path);
    const match = /^bytes=(\d+)-(\d*)$/.exec(request.headers.get("range") ?? "");
    const start = match ? Number(match[1]) : 0;
    const end = match?.[2] ? Math.min(Number(match[2]), file.size - 1) : file.size - 1;
    if (start >= file.size) return new Response(null, { status: 416, headers: { "Content-Range": `bytes */${file.size}` } });
    const bytes = end - start + 1;
    requests.push({ path: url.pathname, method: request.method, roleProbe: url.searchParams.get("role") === "probe",
        ranged: Boolean(match), bytes, userAgent: request.headers.get("user-agent") ?? "" });
    const headers: Record<string, string> = { "Content-Length": String(bytes), "Accept-Ranges": "bytes", ETag: `"legal-fixture-${file.size}"` };
    if (match) headers["Content-Range"] = `bytes ${start}-${end}/${file.size}`;
    return new Response(request.method === "HEAD" ? null : file.slice(start, end + 1), { status: match ? 206 : 200, headers });
} });
try {
    await classify("localAnime", anime, "file", "Anime");
    await classify("localMovie", movie, "file", "Movie");
    await classify("httpAnime", `${server.url}anime.mkv`, "network", "Anime");
    await classify("httpMovie", `${server.url}movie.mkv`, "network", "Movie");
    await classify("hlsAnime", `${server.url}hls/anime.m3u8`, "network", "Anime");
    await classify("rangeAnime", `${server.url}media/0123456789abcdef0123456789abcdef/file`, "network", "Anime");
    await classify("quotedQuery", `${server.url}anime.mkv?value=a,b'c:[x];fixture-private-query`, "network", "Anime");
    const selection = join(evidence, "selection.json");
    const other = `${server.url}never-selected`;
    const selected = `${server.url}movie.mkv`;
    await writeFile(selection, JSON.stringify({ streamFilename: `edl://!no_chapters;${other};!new_stream;${selected}`,
        trackTitle: "chosen muxed-2", extractorResult: { status: 0, stdout: JSON.stringify({ formats: [
            { format: "other", vcodec: "h264", url: other }, { format: "chosen", vcodec: "h264", url: selected },
        ] }) } }));
    await classify("selectedYtdlVideo", selected, "resolve", "Movie", selection);
    for (const mode of ["auth", "cookies"]) {
        const trace = process.platform === "linux" ? join(evidence, `${mode}-exec.trace`) : undefined;
        const command = trace ? ["/usr/bin/strace", "-f", "-qq", "-e", "trace=execve", "-o", trace] : [];
        command.push(fixture, ffprobe, `${server.url}protected`, "16", mode);
        const [program, ...argumentsList] = command;
        const result = await run(program, argumentsList);
        assert.equal(JSON.parse(result.stdout).status, "protected");
        if (trace) {
            const execs = await readFile(trace, "utf8");
            assert(!execs.includes(`execve(${JSON.stringify(ffprobe)},`), "A protected input launched FFprobe");
            assert(!execs.includes("fixture-private-authorization") && !execs.includes("fixture-private-cookie"));
            await unlink(trace);
        }
        results[mode] = { status: "protected", helperExecCount: trace ? 0 : null };
    }
    assert.equal((await classify("cancel", `${server.url}slow`, "cancel")).status, "cancelled");
    for (let attempt = 0; attempt < 50 && !slowClosed; ++attempt) await Bun.sleep(20);
    assert(slowClosed, "Cancelled network reader did not disconnect");
    assert.equal((await classify("privateFailure", `${server.url}missing?token=fixture-private-query,'[x]:y;`, "failure")).status, "failed");
    assert.equal(protectedRequests, 0);
    assert(!credentialsObserved);
    const rangeRequests = requests.filter(request => request.path.startsWith("/media/"));
    assert(rangeRequests.length > 0 && rangeRequests.every(request => request.roleProbe), "Torrent identity lost role=probe");
    assert(requests.filter(request => request.ranged).every(request => request.bytes <= 1024 * 1024));
    assert(requests.every(request => request.userAgent === "qplay-style-fixture: 'quoted', [value]"));
    if (nativeExecutable && mpvLibrary) {
        const profile = join(evidence, "native-profile");
        const prefix = join(evidence, "native-policy");
        await mkdir(profile);
        environment.QPLAY_CONTENT_POLICY_QA = "1";
        environment.QPLAY_CONTENT_MOVIE_QA = `${server.url}movie.mkv`;
        environment.QPLAY_CONTENT_CANCEL_QA = `${server.url}${cancelledRangePath.slice(1)}`;
        await run(resolve(nativeExecutable), [resolve(mpvLibrary), anime, `${server.url}anime.mkv`, profile, prefix], 180000);
        assert(nativeSlowStarted > 0 && nativeSlowClosed, "Replacement media did not cancel an observed native network probe");
        results.nativePolicy = JSON.parse(await readFile(prefix + ".json", "utf8"));
    }
    const files = {} as Record<string, { bytes: number; sha256: string }>;
    for (const path of [anime, movie]) {
        const hash = createHash("sha256");
        for await (const chunk of createReadStream(path)) hash.update(chunk);
        files[basename(path)] = { bytes: Bun.file(path).size, sha256: hash.digest("hex") };
    }
    await writeFile(join(evidence, "receipt.json"), JSON.stringify({ results, requests, files, protectedRequests,
        credentialsObserved, slowClosed, nativeSlowStarted, nativeSlowClosed,
        fullGeneratedMediaDecode: true, framegenPrepared: false }, null, 2) + "\n");
} finally {
    for (const controller of activeStreams) { try { controller.close(); } catch {} }
    server.stop(true);
}
