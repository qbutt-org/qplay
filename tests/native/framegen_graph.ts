import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { delimiter, join, relative, resolve } from "node:path";

const [buildDirectory, vapoursynthLibrary, qtDirectory, evidenceDirectory] = Bun.argv.slice(2);
if (!buildDirectory || !vapoursynthLibrary || !qtDirectory || !evidenceDirectory) {
    throw new Error("Usage: bun tests/native/framegen_graph.ts <build> <native VS library> <Qt SDK> <evidence>");
}
const repository = resolve(import.meta.dir, "../..");
const output = resolve(evidenceDirectory);
const outputFromRepository = relative(repository, output);
if (!outputFromRepository.startsWith("..") && !outputFromRepository.includes(":")) {
    throw new Error("Keep graph acceptance fixtures and evidence outside the source checkout.");
}
await mkdir(join(output, "cache"), { recursive: true });
const windows = process.platform === "win32";
const executable = resolve(buildDirectory, `qplay-framegen-graph-acceptance${windows ? ".exe" : ""}`);
const plugin = resolve(buildDirectory, windows
    ? "qplay-framegen-model-fixture.dll" : "libqplay-framegen-model-fixture.so");
const lock = join(output, "locks/framegen-gpu.lock");
const configuration = join(output, "graph.json");
await writeFile(configuration, JSON.stringify({
    schema_version: 2,
    backend: "openvino",
    source_num: 24000,
    source_den: 1001,
    target_num: 60000,
    target_den: 1001,
    scene_threshold: 0.08917166076596267,
    duplicate_threshold: 0.001,
    matrix: "709",
    range: "limited",
    plugin_path: plugin,
    model_path: plugin,
    engine_path: join(output, "cache"),
    device_id: 0,
    gpu_id: "uuid:00000000000000000000000000000001",
    openvino_device: "GPU.0",
    gpu_lock_path: lock,
}, null, 2));
const child = spawn(executable, [resolve(vapoursynthLibrary), configuration, plugin, lock], {
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
    env: {
        ...process.env,
        PATH: `${resolve(qtDirectory, "bin")}${delimiter}${process.env.PATH ?? ""}`,
    },
});
let log = "";
let timedOut = false;
child.stdout.on("data", (chunk) => { log += chunk.toString(); });
child.stderr.on("data", (chunk) => { log += chunk.toString(); });
const timeout = setTimeout(() => {
    timedOut = true;
    child.kill();
}, 60_000);
const exitCode = await new Promise<number | null>((accept, reject) => {
    child.once("error", reject);
    child.once("close", accept);
}).finally(() => clearTimeout(timeout));
await writeFile(join(output, "graph.log"), log);
if (timedOut || exitCode !== 0) {
    throw new Error(`Native graph acceptance ${timedOut ? "timed out" : `exited ${exitCode}`}\n${log.slice(-6000)}`);
}
console.log(log);
