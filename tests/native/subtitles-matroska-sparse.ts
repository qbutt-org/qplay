import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const [executable, qtDirectory, evidenceFile] = Bun.argv.slice(2);
if (!executable || !qtDirectory || !evidenceFile)
    throw new Error("Usage: bun tests/native/subtitles-matroska-sparse.ts <fixture executable> <Qt SDK> <evidence file>");

function bytes(...items: Uint8Array[]): Uint8Array {
    const output = new Uint8Array(items.reduce((size, item) => size + item.length, 0));
    let offset = 0;
    for (const item of items) {
        output.set(item, offset);
        offset += item.length;
    }
    return output;
}

function unsigned(value: number): Uint8Array {
    const output = new Uint8Array(8);
    new DataView(output.buffer).setBigUint64(0, BigInt(value));
    return output;
}

function size(value: number): Uint8Array {
    const encoded = unsigned(value);
    for (let width = 1; width <= 8; ++width) {
        if (BigInt(value) < (1n << BigInt(width * 7)) - 1n) {
            const result = encoded.slice(8 - width);
            result[0] |= 1 << (8 - width);
            return result;
        }
    }
    throw new Error("Generated EBML size is too large");
}

function element(id: number[], payload: Uint8Array): Uint8Array {
    return bytes(Uint8Array.from(id), size(payload.length), payload);
}

const text = (value: string) => new TextEncoder().encode(value);
const header = element([0x1a, 0x45, 0xdf, 0xa3], bytes(
    element([0x42, 0x82], text("matroska")), element([0x42, 0x87], Uint8Array.of(4)), element([0x42, 0x85], Uint8Array.of(2)),
));
const info = element([0x15, 0x49, 0xa9, 0x66], bytes(
    element([0x4d, 0x80], text("qplay-generated")), element([0x57, 0x41], text("qplay-generated")),
));
const assHeader = "[Script Info]\nScriptType: v4.00+\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize\nStyle: Default,Arial,20\n";
const tracks = element([0x16, 0x54, 0xae, 0x6b], bytes(...[1, 2, 3].map(track => element([0xae], bytes(
    element([0xd7], unsigned(track)), element([0x73, 0xc5], unsigned(track)),
    element([0x83], Uint8Array.of(track === 1 ? 1 : 17)),
    element([0x86], text(track === 1 ? "V_MPEG4/ISO/AVC" : "S_TEXT/ASS")),
    ...(track === 1 ? [] : [element([0x63, 0xa2], text(assHeader))]),
)))));

function subtitle(track: number, duration: number, body: string): Uint8Array {
    return element([0xa0], bytes(
        element([0xa1], bytes(Uint8Array.of(0x80 | track, 0, 0, 0), text(body))),
        element([0x9b], unsigned(duration)),
    ));
}

type Part = { offset: number; data: Uint8Array };
function container(videoBytes: number): { length: number; parts: Part[] } {
    const parts: Part[] = [];
    const seekEntry = (id: number[], offset: number) => element([0x4d, 0xbb], bytes(
        element([0x53, 0xab], Uint8Array.from(id)), element([0x53, 0xac], unsigned(offset)),
    ));
    const seek = (offset: number, cues: number) => element([0x11, 0x4d, 0x9b, 0x74], bytes(
        seekEntry([0x15, 0x49, 0xa9, 0x66], offset),
        seekEntry([0x16, 0x54, 0xae, 0x6b], offset + info.length),
        seekEntry([0x1f, 0x43, 0xb6, 0x75], offset + info.length + tracks.length),
        seekEntry([0x1c, 0x53, 0xbb, 0x6b], cues),
    ));
    const seekHead = seek(seek(0, 0).length, 0);
    let offset = seekHead.length + info.length + tracks.length;
    parts.push({ offset: 0, data: bytes(seekHead, info, tracks) });
    const cuePoints: Uint8Array[] = [];
    const cue = (timestamp: number, clusterPosition: number, relativePosition: number, duration: number) => element([0xbb], bytes(
        element([0xb3], unsigned(timestamp)),
        element([0xb7], bytes(
            element([0xf7], unsigned(3)),
            element([0xf1], unsigned(clusterPosition)),
            element([0xf0], unsigned(relativePosition)),
            element([0xb2], unsigned(duration)),
        )),
    ));

    for (let second = 0; second < 7_200; ++second) {
        const clusterPosition = offset;
        const timestamp = element([0xe7], unsigned(second * 1_000));
        const wrong = second === 1 ? subtitle(2, 6_499_000, "0,0,Default,,0,0,0,,Wrong track") : new Uint8Array();
        const selected = second === 1
            ? subtitle(3, 6_499_000, "0,0,Default,,0,0,0,,{\\an8}Long overlapping cue")
            : second === 6_001 ? subtitle(3, 2_000, "1,0,Default,,0,0,0,,Selected track after seek") : new Uint8Array();
        const subtitleData = second === 1
            ? bytes(wrong, selected)
            : selected;
        const block = bytes(Uint8Array.of(0xa3), size(videoBytes + 4), Uint8Array.of(0x81, 0, 0, 0));
        const cluster = bytes(Uint8Array.of(0x1f, 0x43, 0xb6, 0x75), size(timestamp.length + block.length + videoBytes + subtitleData.length));
        parts.push({ offset, data: bytes(cluster, timestamp, block) });
        offset += cluster.length + timestamp.length + block.length + videoBytes;
        if (subtitleData.length) {
            parts.push({ offset, data: subtitleData });
            offset += subtitleData.length;
        }
        if (selected.length) {
            const relative = timestamp.length + block.length + videoBytes + wrong.length;
            cuePoints.push(cue(second * 1_000, clusterPosition, relative, second === 1 ? 6_499_000 : 2_000));
        }
    }
    const cues = element([0x1c, 0x53, 0xbb, 0x6b], bytes(...cuePoints));
    parts.push({ offset, data: cues });
    const fullSeekHead = seek(seekHead.length, offset);
    if (fullSeekHead.length !== seekHead.length)
        throw new Error("SeekHead size changed");
    parts[0].data = bytes(fullSeekHead, info, tracks);
    offset += cues.length;
    const prefix = bytes(header, Uint8Array.of(0x18, 0x53, 0x80, 0x67), size(offset));
    return { length: prefix.length + offset, parts: [{ offset: 0, data: prefix }, ...parts.map(part => ({ offset: part.offset + prefix.length, data: part.data }))] };
}

const sources = new Map([["sparse.mkv", container(1024 * 1024)]]);
const measurements = new Map<string, { requests: number; receivedBytes: number; representationBytes: number }>();
const server = Bun.serve({
    hostname: "127.0.0.1", port: 0,
    fetch(request) {
        const name = new URL(request.url).pathname.split("/").at(-1)!;
        const source = sources.get(name);
        if (!source)
            return new Response(null, { status: 404 });
        const etag = `"fixture-${name}-${source.length}"`;
        if (request.method === "HEAD")
            return new Response(null, { headers: { "Content-Length": String(source.length), ETag: etag } });
        if (request.headers.get("If-Match") !== etag)
            return new Response(null, { status: 412 });
        const range = /^bytes=(\d+)-(\d+)$/.exec(request.headers.get("Range") ?? "");
        if (!range)
            return new Response(null, { status: 416 });
        const start = Number(range[1]);
        const end = Number(range[2]);
        if (end < start || end >= source.length)
            return new Response(null, { status: 416 });
        const data = new Uint8Array(end - start + 1);
        for (const part of source.parts) {
            if (part.offset > end)
                break;
            const left = Math.max(start, part.offset);
            const right = Math.min(end + 1, part.offset + part.data.length);
            if (left < right)
                data.set(part.data.subarray(left - part.offset, right - part.offset), left - start);
        }
        const measured = measurements.get(name) ?? { requests: 0, receivedBytes: 0, representationBytes: source.length };
        measured.requests++;
        measured.receivedBytes += data.length;
        measurements.set(name, measured);
        return new Response(data, { status: 206, headers: {
            "Content-Length": String(data.length), "Content-Range": `bytes ${start}-${end}/${source.length}`, ETag: etag,
        } });
    },
});

try {
    const source = sources.get("sparse.mkv")!;
    const start = performance.now();
    const url = new URL("media/0123456789abcdef0123456789abcdef/sparse.mkv", server.url).href;
    const child = Bun.spawn([resolve(executable), url, "3", "Selected track after seek", "ass", "6001000", "6005000"], {
        stdout: "pipe", stderr: "pipe", windowsHide: true,
        env: { ...process.env, Path: `${resolve(qtDirectory, "bin")};${process.env.Path ?? process.env.PATH ?? ""}` },
    });
    const deadline = setTimeout(() => child.kill(), 60_000);
    const [stdout, stderr, exitCode] = await Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited])
        .finally(() => clearTimeout(deadline));
    const evidence = { exitCode, elapsedMs: Math.round(performance.now() - start), representationBytes: source.length,
        measurements: Object.fromEntries(measurements), results: stdout.trim().split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line)), stderr };
    await writeFile(resolve(evidenceFile), JSON.stringify(evidence, null, 4));
    if (exitCode !== 0)
        throw new Error(`Range acquisition fixture failed: ${stderr}`);
    console.log(JSON.stringify(evidence, null, 4));
} finally {
    server.stop(true);
}
