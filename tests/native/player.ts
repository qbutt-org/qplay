import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { copyFile, mkdir, readFile, unlink, writeFile } from "node:fs/promises";
import { createReadStream } from "node:fs";
import { delimiter, join, relative, resolve } from "node:path";

const [buildDirectory, runtimeDirectory, mediaFile, qtDirectory, evidenceDirectory] = Bun.argv.slice(2);

if (!buildDirectory || !runtimeDirectory || !mediaFile || !qtDirectory || !evidenceDirectory) {
    throw new Error("Usage: bun tests/native/player.ts <build> <qplay runtime> <legal media> <Qt SDK> <evidence>");
}

const repository = resolve(import.meta.dir, "../..");
const output = resolve(evidenceDirectory);
const outputFromRepository = relative(repository, output);

if (!outputFromRepository.startsWith("..") && !outputFromRepository.includes(":")) {
    throw new Error("Keep generated acceptance evidence outside the source checkout.");
}

const windows = process.platform === "win32";
const executable = resolve(buildDirectory, `qplay-native-acceptance${windows ? ".exe" : ""}`);
const library = resolve(runtimeDirectory, windows ? "engine/libmpv-2.dll" : "engine/libmpv.so.2");
const profile = join(output, `профиль-${randomUUID()}`);
const evidencePrefix = join(output, "player");

await mkdir(profile, { recursive: true });
const secondMedia = join(profile, "второй-sintel.mp4");
await copyFile(resolve(mediaFile), secondMedia);
const argumentsList = [library, resolve(mediaFile), secondMedia, profile, evidencePrefix];
const child = spawn(executable, argumentsList, {
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
    env: {
        ...process.env,
        PATH: `${resolve(qtDirectory, "bin")}${delimiter}${process.env.PATH ?? ""}`,
        QT_PLUGIN_PATH: resolve(qtDirectory, "plugins"),
        QML_IMPORT_PATH: resolve(qtDirectory, "qml"),
        QT_COMMAND_LINE_PARSER_NO_GUI_MESSAGE_BOXES: "1",
    },
});

let log = "";
let timedOut = false;
child.stdout.on("data", (chunk) => {
    log += chunk.toString();
});
child.stderr.on("data", (chunk) => {
    log += chunk.toString();
    if (process.env.QPLAY_NATIVE_FRAMEGEN_ACCEPTANCE)
        process.stderr.write(chunk);
});

const timeout = setTimeout(() => {
    timedOut = true;
    child.kill();
}, process.env.QPLAY_NATIVE_FRAMEGEN_ACCEPTANCE ? 35 * 60_000 : 60_000);

const exitCode = await new Promise<number | null>((accept, reject) => {
    child.once("error", reject);
    child.once("close", accept);
}).finally(() => clearTimeout(timeout));

await unlink(secondMedia);
await writeFile(join(output, "player.log"), log);

if (timedOut || exitCode !== 0) {
    throw new Error(`Native player acceptance ${timedOut ? "timed out" : `exited ${exitCode}`}\n${log.slice(-6000)}`);
}

const evidence = JSON.parse(await readFile(`${evidencePrefix}.json`, "utf8"));
const startupQa = Boolean(process.env.QPLAY_STARTUP_RENDER_QA);
const extractorQa = Boolean(process.env.QPLAY_EXTRACTOR_ERROR_QA);
const framegenQa = Boolean(process.env.QPLAY_NATIVE_FRAMEGEN_ACCEPTANCE);

const digest = async (path: string) => createHash("sha256").update(await readFile(path)).digest("hex");
let accepted: boolean;
let renders: Record<string, string>;

if (framegenQa) {
    accepted = evidence.framegenActive && evidence.framegenRenderedVideoChanged
        && evidence.manualGpuChoicesPersisted && evidence.framegenRestoredAfterManualShader
        && evidence.ownedChildren === 0;
    renders = { framegenSha256: await digest(`${evidencePrefix}.framegen.png`) };
    const probe = Bun.spawn([
        resolve(runtimeDirectory, `engine/ffprobe${windows ? ".exe" : ""}`),
        "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "json",
        resolve(mediaFile),
    ], { stdout: "pipe", stderr: "pipe", timeout: 15_000 });
    const [source, probeError, probeExit] = await Promise.all([
        new Response(probe.stdout).json(), new Response(probe.stderr).text(), probe.exited,
    ]);
    if (probeExit !== 0 || evidence.framegenOutputWidth !== source.streams?.[0]?.width
        || evidence.framegenOutputHeight !== source.streams?.[0]?.height) {
        throw new Error(`Native frame generation changed decoded geometry: ${probeError}`);
    }
    const lock = JSON.parse(await readFile(join(repository, "upstream-lock.json"), "utf8"));
    const runtime = windows ? lock.runtime : lock.linux.runtime;
    const enginePins = new Map(Object.entries(runtime.files)
        .map(([path, hash]) => [windows ? path.toLowerCase() : path, hash]));
    const enginePath = resolve(runtimeDirectory, "engine");
    const engine = windows ? enginePath.toLowerCase() : enginePath;
    if (!evidence.loadedLibraries?.length)
        throw new Error("The native process did not report its loaded libraries.");
    const libraries = [];
    for (const path of evidence.loadedLibraries) {
        const hash = createHash("sha256");
        for await (const chunk of createReadStream(path)) hash.update(chunk);
        const sha256 = hash.digest("hex");
        const libraryPath = resolve(path);
        const withinEngine = relative(engine, windows ? libraryPath.toLowerCase() : libraryPath);
        if (!withinEngine.startsWith("..") && !withinEngine.includes(":")) {
            const expected = enginePins.get(withinEngine.replaceAll("\\", "/"));
            if (!expected || sha256 !== expected) {
                throw new Error(`The loaded product engine dependency differs from its pin: ${path}`);
            }
        }
        libraries.push({ path, sha256 });
    }
    evidence.loadedLibraries = libraries;
} else if (extractorQa) {
    accepted = evidence.extractorReason && evidence.privateDataRedacted && evidence.boundedBytes <= 1024
        && evidence.errorSuccessNewError && evidence.newGenerationReason && evidence.ownedChildren === 0;
    renders = { errorFrameSha256: await digest(`${evidencePrefix}.error.png`) };
} else if (startupQa) {
    accepted = evidence.mediaWaitedForRender && evidence.ipcWaitedForRender
        && evidence.startupVideo?.w > 0 && evidence.ipcReplacementVideo?.w > 0;
    renders = {};
} else {
    accepted = evidence.pauseSeekResume && evidence.playbackMenuOpened
        && evidence.speedSubmenuOpened && evidence.speedPresetApplied
        && evidence.settingsLightRendered && evidence.settingsDarkRendered
        && evidence.closeDuringStartup && evidence.ownedChildren === 0;
    renders = {
        frameSha256: await digest(`${evidencePrefix}.png`),
        menuFrameSha256: await digest(`${evidencePrefix}.menu.png`),
        speedFrameSha256: await digest(`${evidencePrefix}.speed.png`),
        settingsLightFrameSha256: await digest(`${evidencePrefix}.settings-light.png`),
        settingsDarkFrameSha256: await digest(`${evidencePrefix}.settings-dark.png`),
    };
}

if (evidence.exitCode !== 0 || !accepted) {
    throw new Error("The native process did not confirm the required playback and lifetime scenario.");
}

const receipt = {
    command: [executable, ...argumentsList],
    engineSha256: await digest(library),
    executableSha256: await digest(executable),
    mediaSha256: await digest(resolve(mediaFile)),
    ...renders,
    evidence,
};

await writeFile(join(output, "receipt.json"), `${JSON.stringify(receipt, null, 4)}\n`);
console.log(JSON.stringify({ evidence: join(output, "receipt.json"), ...evidence }, null, 4));
