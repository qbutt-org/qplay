import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const [executable, qtDirectory, evidenceFile, cancelExecutable] = Bun.argv.slice(2);
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

function subtitle(track: number, duration: number, body: string): Uint8Array {
    return element([0xa0], bytes(
        element([0xa1], bytes(Uint8Array.of(0x80 | track, 0, 0, 0), text(body))),
        element([0x9b], unsigned(duration)),
    ));
}

type Part = { offset: number; data: Uint8Array };
type ContainerOptions = {
    index?: "subtitles" | "video" | "none";
    clusterCount?: number;
    selectedSecond?: number;
    unknownClusterWidth?: number;
    videoTrack?: number;
    subtitleTrack?: number;
    fault?: "track" | "size";
};

function container(videoBytes: number, {
    index = "subtitles", clusterCount = 7_200, selectedSecond = 6_001,
    unknownClusterWidth = 0, videoTrack = 1, subtitleTrack = 3, fault,
}: ContainerOptions = {}): { length: number; parts: Part[] } {
    const tracks = element([0x16, 0x54, 0xae, 0x6b], bytes(...[videoTrack, 2, subtitleTrack].map(track => element([0xae], bytes(
        element([0xd7], unsigned(track)), element([0x73, 0xc5], unsigned(track)),
        element([0x83], Uint8Array.of(track === videoTrack ? 1 : 17)),
        element([0x86], text(track === videoTrack ? "V_MPEG4/ISO/AVC" : "S_TEXT/ASS")),
        ...(track === videoTrack ? [] : [element([0x63, 0xa2], text(assHeader))]),
    )))));
    const parts: Part[] = [];
    const seekEntry = (id: number[], offset: number) => element([0x4d, 0xbb], bytes(
        element([0x53, 0xab], Uint8Array.from(id)), element([0x53, 0xac], unsigned(offset)),
    ));
    const seek = (offset: number, cues: number) => element([0x11, 0x4d, 0x9b, 0x74], bytes(
        seekEntry([0x15, 0x49, 0xa9, 0x66], offset),
        seekEntry([0x16, 0x54, 0xae, 0x6b], offset + info.length),
        seekEntry([0x1f, 0x43, 0xb6, 0x75], offset + info.length + tracks.length),
        ...(index !== "none" ? [seekEntry([0x1c, 0x53, 0xbb, 0x6b], cues)] : []),
    ));
    const seekHead = seek(seek(0, 0).length, 0);
    let offset = seekHead.length + info.length + tracks.length;
    parts.push({ offset: 0, data: bytes(seekHead, info, tracks) });
    const cuePoints: Uint8Array[] = [];
    const cue = (timestamp: number, clusterPosition: number, relativePosition: number, duration: number) => element([0xbb], bytes(
        element([0xb3], unsigned(timestamp)),
        element([0xb7], bytes(
            element([0xf7], unsigned(index === "video" ? videoTrack : subtitleTrack)),
            element([0xf1], unsigned(clusterPosition)),
            element([0xf0], unsigned(relativePosition)),
            element([0xb2], unsigned(duration)),
        )),
    ));

    for (let second = 0; second < clusterCount; ++second) {
        const clusterPosition = offset;
        const timestamp = element([0xe7], unsigned(second * 1_000));
        const wrong = second === 1 ? subtitle(2, 6_499_000, "0,0,Default,,0,0,0,,Wrong track") : new Uint8Array();
        let selected = new Uint8Array();
        if (second === 1)
            selected = subtitle(subtitleTrack, 6_499_000, "0,0,Default,,0,0,0,,{\\an8}Long overlapping cue");
        else if (second === selectedSecond)
            selected = subtitle(fault === "track" ? 0 : subtitleTrack, 2_000, "1,0,Default,,0,0,0,,Selected track after seek");
        const malformed = second === selectedSecond && fault === "size"
            ? element([0xa0], bytes(Uint8Array.of(0xa1, 0x89, 0x83, 0, 0, 0))) : new Uint8Array();
        const subtitleData = bytes(second === 1 ? wrong : new Uint8Array(), selected, malformed);
        const block = bytes(Uint8Array.of(0xa3), size(videoBytes + 4), Uint8Array.of(0x80 | videoTrack, 0, 0, 0));
        const clusterSize = unknownClusterWidth
            ? new Uint8Array(unknownClusterWidth).fill(0xff)
            : size(timestamp.length + block.length + videoBytes + subtitleData.length);
        if (unknownClusterWidth)
            clusterSize[0] >>>= unknownClusterWidth - 1;
        const cluster = bytes(Uint8Array.of(0x1f, 0x43, 0xb6, 0x75), clusterSize);
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
    const cues = index !== "none" ? element([0x1c, 0x53, 0xbb, 0x6b], bytes(...cuePoints)) : new Uint8Array();
    parts.push({ offset, data: cues });
    const fullSeekHead = seek(seekHead.length, offset);
    if (fullSeekHead.length !== seekHead.length)
        throw new Error("SeekHead size changed");
    parts[0].data = bytes(fullSeekHead, info, tracks);
    offset += cues.length;
    const prefix = bytes(header, Uint8Array.of(0x18, 0x53, 0x80, 0x67), size(offset));
    return { length: prefix.length + offset, parts: [{ offset: 0, data: prefix }, ...parts.map(part => ({ offset: part.offset + prefix.length, data: part.data }))] };
}

const shortLayout = { clusterCount: 128, selectedSecond: 121 };
const unindexed = container(1024 * 1024, { ...shortLayout, index: "none" });
const sources = new Map([
    ["sparse.mkv", container(1024 * 1024)],
    ["unindexed.mkv", unindexed],
    ["known-size-127.mkv", container(111, { ...shortLayout, index: "none" })],
    ["video-index.mkv", container(1024 * 1024, { ...shortLayout, index: "video" })],
    ["unknown-clusters.mkv", container(1024 * 1024, { ...shortLayout, index: "none", unknownClusterWidth: 1 })],
    ["changed.mkv", unindexed],
    ["slow.mkv", unindexed],
    ["track.mkv", container(1024 * 1024, { ...shortLayout, index: "none", fault: "track" })],
    ["size.mkv", container(1024 * 1024, { ...shortLayout, index: "none", fault: "size" })],
    ["budget.mkv", container(1024 * 1024, { clusterCount: 600, selectedSecond: 121, index: "none" })],
]);
for (let width = 2; width <= 8; width++) {
    sources.set(`unknown-clusters-${width}.mkv`, container(1024 * 1024, {
        ...shortLayout, index: "none", unknownClusterWidth: width,
    }));
}
sources.set("video-track-127.mkv", container(1024 * 1024, {
    ...shortLayout, index: "none", videoTrack: 127,
}));
sources.set("subtitle-track-127.mkv", container(1024 * 1024, {
    ...shortLayout, index: "none", subtitleTrack: 127,
}));
let slowClosed = false;
const measurements = new Map<string, { requests: number; receivedBytes: number; representationBytes: number }>();
const server = Bun.serve({
    hostname: "127.0.0.1", port: 0,
    async fetch(request) {
        const name = new URL(request.url).pathname.split("/").at(-1)!;
        const source = sources.get(name);
        if (!source)
            return new Response(null, { status: 404 });
        const etag = '"fixture-' + name + '-' + source.length + '"';
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
        if (name === "slow.mkv" && start > 65_536) {
            await new Promise<void>(resolve => request.signal.addEventListener("abort", () => {
                slowClosed = true;
                resolve();
            }, { once: true }));
            return new Response(null, { status: 499 });
        }
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
            "Content-Length": String(data.length),
            "Content-Range": `bytes ${start}-${end}/${source.length}`,
            ETag: name === "changed.mkv" && measured.requests > 2 ? '"changed-during-scan"' : etag,
        } });
    },
});

try {
    const cases: unknown[] = [];
    const profile = resolve(evidenceFile + ".profile");
    const run = async (name: string, expected: string, start: number, end: number, {
        mode, token = "0123456789abcdef0123456789abcdef", track = 3,
    }: { mode?: "seed" | "offline"; token?: string; track?: number } = {}) => {
        const url = new URL(`media/${token}/${name}`, server.url).href;
        const child = Bun.spawn([resolve(executable), url, String(track), expected, "ass", String(start), String(end),
            ...(mode ? [profile, mode] : [])], {
            stdout: "pipe", stderr: "pipe", windowsHide: true,
            env: { ...process.env, Path: `${resolve(qtDirectory, "bin")};${process.env.Path ?? process.env.PATH ?? ""}` },
        });
        const deadline = setTimeout(() => child.kill(), 60_000);
        const [stdout, stderr, exitCode] = await Promise.all([
            new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited,
        ]).finally(() => clearTimeout(deadline));
        const result = stdout.trim().split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
        cases.push({ name, mode, exitCode, result, stderr });
        if (exitCode !== 0 || !result.length)
            throw new Error(`Range acquisition fixture failed for ${name}: ${stderr}`);
        return result[0];
    };
    await run("sparse.mkv", "Selected track after seek", 6_001_000, 6_005_000);
    await run("known-size-127.mkv", "Selected track after seek", 121_000, 125_000);
    await run("video-index.mkv", "Selected track after seek", 121_000, 125_000);
    await run("unknown-clusters.mkv", "Selected track after seek", 121_000, 125_000);
    for (let width = 2; width <= 8; width++)
        await run(`unknown-clusters-${width}.mkv`, "Selected track after seek", 121_000, 125_000);
    await run("video-track-127.mkv", "Selected track after seek", 121_000, 125_000);
    await run("subtitle-track-127.mkv", "Selected track after seek", 121_000, 125_000, { track: 127 });
    const seeded = await run("unindexed.mkv", "Selected track after seek", 121_000, 125_000, { mode: "seed" });
    const reopened = await run("unindexed.mkv", "Selected track after seek", 121_000, 125_000, { mode: "offline", token: "fedcba9876543210fedcba9876543210" });
    if (seeded.mediaDigest !== reopened.mediaDigest || seeded.sourceDigest !== reopened.sourceDigest)
        throw new Error("Capability URL changed exact cache identity");
    for (const name of ["changed.mkv", "track.mkv", "size.mkv", "budget.mkv"]) {
        const result = await run(name, "--reject", 121_000, 125_000);
        if (result.status !== "rejected")
            throw new Error(`Malformed representation accepted: ${name}`);
    }
    if (cancelExecutable) {
        const url = new URL("media/0123456789abcdef0123456789abcdef/slow.mkv", server.url).href;
        const child = Bun.spawn([resolve(cancelExecutable), url], {
            stdout: "pipe", stderr: "pipe", windowsHide: true,
            env: { ...process.env, Path: `${resolve(qtDirectory, "bin")};${process.env.Path ?? process.env.PATH ?? ""}` },
        });
        const deadline = setTimeout(() => child.kill(), 10_000);
        const [stdout, stderr, exitCode] = await Promise.all([
            new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited,
        ]).finally(() => clearTimeout(deadline));
        for (let attempt = 0; attempt < 20 && !slowClosed; attempt++) await Bun.sleep(50);
        cases.push({ name: "pending-scan-cancel", exitCode, stdout, stderr, slowClosed });
        if (exitCode !== 0 || !slowClosed)
            throw new Error(`Pending scan did not drain cancellation: ${stderr}`);
    }
    for (const measured of measurements.values()) {
        if (measured.receivedBytes > 64 * 1024 * 1024)
            throw new Error("Acquisition exceeded its per-operation byte budget");
    }
    await writeFile(resolve(evidenceFile), JSON.stringify({ cases, slowClosed, measurements: Object.fromEntries(measurements) }, null, 4));
    console.log(`Sparse/without-Cues acquisition PASS: ${evidenceFile}`);
} finally {
    server.stop(true);
}
