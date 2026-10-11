import { spawn, type ChildProcess } from "node:child_process";
import { cp, mkdir, mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { createConnection, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";

const { values } = parseArgs({ options: {
    bundle: { type: "string", default: join(tmpdir(), "qplay-build", "portable") },
    output: { type: "string" },
} });
const bundle = await realpath(resolve(values.bundle!));
const output = values.output ? resolve(values.output) : await mkdtemp(join(tmpdir(), "qplay-smoke-"));
await mkdir(output, { recursive: true });
const evidence = await realpath(output);
const executable = join(bundle, "qplay.exe");

async function command(args: string[], cwd = bundle): Promise<{ code: number; stdout: string; stderr: string }> {
    const process = Bun.spawn(args, { cwd, stdout: "pipe", stderr: "pipe" });
    const timeout = setTimeout(() => process.kill(), 30_000);
    try {
        const [stdout, stderr, code] = await Promise.all([
            new Response(process.stdout).text(), new Response(process.stderr).text(), process.exited
        ]);
        return { code, stdout, stderr };
    }
    finally { clearTimeout(timeout); }
}

function check(condition: unknown, message: string): asserts condition {
    if (!condition)
        throw new Error(message);
}

const version = await command([executable, "--version-json"]);
check(version.code === 0, version.stderr);
check(JSON.parse(version.stdout).product === "qplay", "Launcher version metadata differs.");
const profile = join(evidence, "Профиль qplay");
const runtimeResult = await command([executable, "--runtime-info-json", `--profile-dir=${profile}`]);
check(runtimeResult.code === 0, runtimeResult.stderr);
const runtime = JSON.parse(runtimeResult.stdout);
check(runtime.protocolVersion === 1 && runtime.tools.mpv && runtime.tools.ffprobe, "Incomplete runtime metadata.");
check(runtime.configDir === join(profile, "config"), "Redirected JSON did not preserve Unicode profile paths.");
for (const option of ["--include=unowned.conf", "--no-load-scripts", "--scripts-append=unowned.lua"]) {
    const rejected = await command([executable, "--version-json", option]);
    check(rejected.code !== 0 && /configuration|конфигурацией/i.test(rejected.stderr), "A caller replaced the owned profile or scripts.");
}
const media = join(evidence, "Материал для qplay.mkv");
const subtitles = join(evidence, "English subtitles.srt");
const positionedSubtitles = join(evidence, "Positioned subtitles.ass");
await writeFile(subtitles, "1\n00:00:00,000 --> 00:00:08,000\nEnglish soft subtitles for translation and positioning.\n");
await writeFile(positionedSubtitles, `[Script Info]
ScriptType: v4.00+
PlayResX: 320
PlayResY: 180

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Segoe UI,16,&H0000FFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,12,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:08.00,Default,,0,0,0,,{\\pos(160,45)}Positioned ASS keeps its font and color.
`);
const generate = await command([runtime.tools.ffmpeg, "-hide_banner", "-loglevel", "error", "-y",
    "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=24", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000",
    "-i", subtitles, "-i", positionedSubtitles, "-map", "0:v:0", "-map", "1:a:0", "-map", "2:0", "-map", "3:0",
    "-t", "8", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "-c:s:0", "srt", "-c:s:1", "ass",
    "-metadata:s:s:0", "language=eng", "-metadata:s:s:0", "title=English [SRT]",
    "-metadata:s:s:1", "language=eng", "-metadata:s:s:1", "title=Positioned ASS", media]);
check(generate.code === 0, generate.stderr);
const probe = await command([runtime.tools.ffprobe, "-v", "error", "-show_entries", "stream=codec_type", "-of", "json", media]);
check(probe.code === 0 && JSON.parse(probe.stdout).streams.length === 4, "The absolute probe path failed outside its engine directory.");
const downloaderVersion = await command([runtime.tools.ytDlp, "--version"]);
check(downloaderVersion.code === 0 && downloaderVersion.stdout.trim(), "The absolute downloader path failed outside its engine directory.");
await mkdir(join(profile, "config"), { recursive: true });
const preferences = { language: "ru", resume: true, frameGeneration: false, subtitlePosition: 67 };
await writeFile(join(profile, "config/settings.json"), JSON.stringify(preferences));
const profileBefore = await readFile(join(profile, "config/settings.json"), "utf8");

async function connect(pipe: string): Promise<Socket> {
    const deadline = Date.now() + 20_000;
    while (Date.now() < deadline) {
        const socket = createConnection(pipe);
        if (await new Promise<boolean>(resolve => {
            socket.once("connect", () => resolve(true));
            socket.once("error", () => resolve(false));
        }))
            return socket;
        socket.destroy();
        await Bun.sleep(100);
    }
    throw new Error("Timed out connecting to actual mpv IPC.");
}

function ipcClient(socket: Socket) {
    let text = "";
    let id = 0;
    const pending = new Map<number, { resolve: (value: any) => void; timeout: ReturnType<typeof setTimeout> }>();
    socket.setEncoding("utf8");
    socket.on("data", chunk => {
        text += chunk;
        let end;
        while ((end = text.indexOf("\n")) !== -1) {
            const message = JSON.parse(text.slice(0, end));
            text = text.slice(end + 1);
            const request = pending.get(message.request_id);
            if (request) {
                clearTimeout(request.timeout);
                pending.delete(message.request_id);
                request.resolve(message);
            }
        }
    });
    return (command: unknown[]) => new Promise<any>((resolve, reject) => {
        const requestId = ++id;
        const timeout = setTimeout(() => {
            pending.delete(requestId);
            reject(new Error("mpv IPC response timed out."));
        }, 5000);
        pending.set(requestId, { resolve, timeout });
        socket.write(JSON.stringify({ command, request_id: requestId }) + "\n");
    });
}

const children = new Set<ChildProcess>();
const sockets = new Set<Socket>();

async function start(name: string) {
    const pipe = `\\\\.\\pipe\\qplay-smoke-${crypto.randomUUID()}`;
    const log = join(evidence, `${name}.log`);
    const child = spawn(executable, [`--profile-dir=${profile}`, `--input-ipc-server=${pipe}`,
        "--vo=null", "--ao=null", "--force-window=no", "--pause=yes", "--keep-open=no", "--idle=yes",
        `--log-file=${log}`, "--", media], { cwd: bundle, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
    children.add(child);
    child.stdout!.resume();
    child.stderr!.resume();
    const exit = new Promise<number | null>((resolve, reject) => {
        child.once("error", reject);
        child.once("exit", code => { children.delete(child); resolve(code); });
    });
    const socket = await connect(pipe);
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
    return { child, exit, socket, request: ipcClient(socket), log };
}

try {
    const first = await start("playback");
    const trackReply = await first.request(["get_property", "track-list"]);
    check(trackReply.error === "success" && trackReply.data.some((track: any) => track.type === "video")
        && trackReply.data.some((track: any) => track.type === "audio")
        && trackReply.data.filter((track: any) => track.type === "sub").length === 2,
        "Legal fixture did not open its audio/video and soft-subtitle tracks.");
    check((await first.request(["get_property", "sub-pos"])).data === 67, "The saved subtitle position did not apply.");
    check(first.child.exitCode === null, "Launcher detached from its active engine.");
    await first.request(["set_property", "pause", false]);
    await Bun.sleep(1800);
    const position = await first.request(["get_property", "time-pos"]);
    check(position.data > 1, "Actual player did not advance decoded playback.");
    await first.request(["set_property", "pause", true]);
    await Bun.sleep(100);
    check((await first.request(["get_property", "sub-pos"])).data === 67,
        "Showing ModernX on pause reset the explicit subtitle position.");
    const languageUpdate = await first.request(["set_property", "user-data/qplay-language", "en"]);
    const language = await first.request(["get_property", "user-data/qplay-language"]);
    check(languageUpdate.error === "success" && language.data === "en", "The player language property did not update.");
    check(await readFile(join(profile, "config/settings.json"), "utf8") === profileBefore, "Playback rewrote explicit user preferences.");
    await first.request(["set_property", "sub-pos", 38]);
    await first.request(["set_property", "user-data/qplay-subtitle-position", 38]);
    await first.request(["script-message", "qplay-save-subtitle-position", "38"]);
    const savedDeadline = Date.now() + 5000;
    let savedPreferences;
    do {
        savedPreferences = JSON.parse(await readFile(join(profile, "config/settings.json"), "utf8"));
        if (savedPreferences.subtitlePosition === 38)
            break;
        await Bun.sleep(50);
    } while (Date.now() < savedDeadline);
    check(savedPreferences.subtitlePosition === 38 && savedPreferences.language === preferences.language
        && savedPreferences.resume === preferences.resume && savedPreferences.frameGeneration === preferences.frameGeneration,
        "The actual player save action failed or replaced unrelated preferences.");
    await first.request(["quit"]);
    check(await first.exit === 0, "Normal engine exit did not reach the launcher.");
    first.socket.destroy();
    const playbackLog = await readFile(first.log, "utf8");
    check(!/Lua error|stack traceback|Error parsing|failed to load|attempt to index/i.test(playbackLog), "A prepared player script failed.");

    const second = await start("job-lifetime");
    check((await second.request(["get_property", "sub-pos"])).data === 38, "Reopening lost the explicit subtitle position.");
    const pidResult = await command(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command",
        `(Get-CimInstance Win32_Process -Filter 'ParentProcessId=${second.child.pid} AND Name="mpv.exe"').ProcessId`]);
    const enginePid = Number(pidResult.stdout.trim());
    check(pidResult.code === 0 && enginePid > 0, "Could not identify the actual owned engine.");
    second.child.kill();
    await second.exit;
    second.socket.destroy();
    await Bun.sleep(300);
    const alive = await command(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command",
        `if (Get-Process -Id ${enginePid} -ErrorAction SilentlyContinue) { exit 1 }`]);
    check(alive.code === 0, "The engine survived forced launcher termination.");
    const reset = await command([executable, `--profile-dir=${profile}`, "--save-subtitle-position=auto"]);
    check(reset.code === 0 && JSON.parse(await readFile(join(profile, "config/settings.json"), "utf8")).subtitlePosition === null,
        "Automatic subtitle positioning did not clear the explicit preference.");

    const corrupt = join(evidence, "corrupt-bundle");
    try {
        await cp(bundle, corrupt, { recursive: true });
        await writeFile(join(corrupt, "engine/profile/mpv.conf"), "corrupt legal fixture\n");
        const rejected = await command([join(corrupt, "qplay.exe"), "--runtime-info-json"], corrupt);
        check(rejected.code !== 0 && /integrity|целостности/i.test(rejected.stderr), "Runtime corruption was accepted.");
    }
    finally { await rm(corrupt, { recursive: true, force: true }); }
    await writeFile(join(evidence, "evidence.json"), JSON.stringify({
        product: "qplay", version: JSON.parse(version.stdout), runtime,
        checks: { redirectedGuiMetadata: true, unicodeProfileMetadata: true, callerProfileBoundary: true,
            immutableRuntime: true, helpersIndependentOfCwd: true, legalVideoAudio: true,
            decodedPosition: position.data, preferencesPreserved: true, playerLanguageProperty: true,
            normalExit: true, forcedExitDrainsEngine: true, damagedRuntimeRejected: true,
            softSubtitleTracks: true, subtitlePositionPersists: true, oscPreservesSubtitlePosition: true,
            subtitlePositionReset: true },
        files: { media, subtitles, positionedSubtitles, playbackLog: first.log, jobLog: second.log }
    }, null, 4) + "\n");
    console.log(JSON.stringify({ evidence, decodedPosition: position.data, checks: 16 }, null, 4));
}
finally {
    for (const socket of sockets)
        socket.destroy();
    for (const child of children)
        child.kill();
}
