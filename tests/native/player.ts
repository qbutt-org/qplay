import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { copyFile, mkdir, readFile, unlink, writeFile } from "node:fs/promises";
import { join, relative, resolve } from "node:path";

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

const executable = resolve(buildDirectory, "qplay-native-acceptance.exe");
const library = resolve(runtimeDirectory, "engine/libmpv-2.dll");
const profile = join(output, `profile-${randomUUID()}`);
const evidencePrefix = join(output, "player");

await mkdir(profile, { recursive: true });
const secondMedia = join(profile, "second-sintel.mp4");
await copyFile(resolve(mediaFile), secondMedia);
const argumentsList = [library, resolve(mediaFile), secondMedia, profile, evidencePrefix];
const child = spawn(executable, argumentsList, {
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
    env: {
        ...process.env,
        PATH: `${resolve(qtDirectory, "bin")};${process.env.PATH ?? ""}`,
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
});

const timeout = setTimeout(() => {
    timedOut = true;
    child.kill();
}, 60_000);

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

if (evidence.exitCode !== 0 || !evidence.pauseSeekResume || !evidence.playbackMenuOpened
    || !evidence.speedSubmenuOpened || !evidence.speedPresetApplied
    || !evidence.settingsLightRendered || !evidence.settingsDarkRendered
    || !evidence.closeDuringStartup || evidence.ownedChildren !== 0) {
    throw new Error("The native process did not confirm the required playback and lifetime scenario.");
}

const digest = async (path: string) => createHash("sha256").update(await readFile(path)).digest("hex");
const receipt = {
    command: [executable, ...argumentsList],
    engineSha256: await digest(library),
    executableSha256: await digest(executable),
    mediaSha256: await digest(resolve(mediaFile)),
    frameSha256: await digest(`${evidencePrefix}.png`),
    menuFrameSha256: await digest(`${evidencePrefix}.menu.png`),
    speedFrameSha256: await digest(`${evidencePrefix}.speed.png`),
    settingsLightFrameSha256: await digest(`${evidencePrefix}.settings-light.png`),
    settingsDarkFrameSha256: await digest(`${evidencePrefix}.settings-dark.png`),
    evidence,
};

await writeFile(join(output, "receipt.json"), `${JSON.stringify(receipt, null, 4)}\n`);
console.log(JSON.stringify({ evidence: join(output, "receipt.json"), ...evidence }, null, 4));
