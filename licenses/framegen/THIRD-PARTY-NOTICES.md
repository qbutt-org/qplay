# Frame generation third-party notices

Frame generation uses the pinned native libraries and models listed below. Every component remains under its own license and is not relicensed by qplay.

The installed runtime keeps this index and every applicable license or provenance file in its `notices` directory. Native VapourSynth keeps the license file from its original archive. NVIDIA TensorRT keeps the exact `LICENSE.txt` from its pinned wheel.

## Common components

- VapourSynth R79 — LGPL-2.1: <https://github.com/vapoursynth/vapoursynth/tree/R79>. Installed copy: `vapoursynth-79.dist-info/licenses/COPYING.LESSER` on Windows or `licenses/VapourSynth-LGPL-2.1.txt` on Linux.
- vs-mlrt v15.16 native plugins — GPL-3.0: <https://github.com/AmusementClub/vs-mlrt/tree/v15.16>. Installed copy: `notices/vs-mlrt-GPL-3.0.txt`.
- RIFE 4.26 models — Practical-RIFE model provenance and MIT terms: <https://github.com/hzwer/Practical-RIFE>. Installed copy: `notices/Practical-RIFE-MIT.txt`.
- vs-rife model-conversion provenance — MIT: <https://github.com/HolyWu/vs-rife>. Installed copy: `notices/vs-rife-MIT.txt`.

## NVIDIA backend

- TensorRT 10.16.0.72 is downloaded from NVIDIA's official Python package index under the NVIDIA TensorRT SLA: <https://docs.nvidia.com/deeplearning/tensorrt/10.x.x/reference/sla.html>. `notices/TensorRT-LICENSE.txt` is extracted byte-for-byte from the pinned wheel.
- The native TensorRT builder uses the pinned NVIDIA/TensorRT OSS v10.16 interfaces: <https://github.com/NVIDIA/TensorRT/tree/v10.16>. Apache-2.0 and NVIDIA's upstream NOTICE are installed as `notices/TensorRT-OSS-LICENSE.txt` and `notices/TensorRT-OSS-NOTICE.txt`.
- The regular RIFE 4.26 ONNX model is downloaded directly from the pinned vs-mlrt external-model release.
- Linux native libraries also carry `licenses/zimg-copyright.txt` and `licenses/CUDA-EULA.txt`.

## Intel backend

- OpenVINO 2026.2.1-21919 is downloaded as the pinned official wheel, and only the required GPU runtime subset is installed. Apache-2.0 plus the exact runtime, oneTBB and oneDNN inventories from that wheel are installed under the `OpenVINO-*` names.
- `tools/bin/framegen-vsov.dll` is the repository's reproducible modification of vs-mlrt v15.16 for OpenVINO 2026.2.1. It is GPL-3.0 and is accompanied in the same repository by complete build inputs and Corresponding Source under `tools/framegen-vsov/`, including the 2026-08-15 modification notice, both VapourSynth R54 interface headers, the pinned vcpkg port snapshot, immutable source directions, and all static-dependency licenses. The runtime installs exact copies of its `VSOV-*` provenance and license files.
- `framegen/models/openvino/rife_v2/rife_v4.26.onnx` is the tracked Intel motion-s0.5/full-resolution-warp variant derived from the MIT-licensed Practical-RIFE 4.26 model. Its auditable rewrite and exact provenance are under `tools/framegen-models/` and are copied into the runtime notices.

TensorRT, OpenVINO and the regular RIFE model are obtained from the pinned publisher downloads. The tracked Intel VSOV object and model are conveyed with the source, provenance, modification notice, and license material named above. If an installed runtime is copied separately, the applicable notices must accompany it and GPL/LGPL source obligations must still be satisfied; ordinary provenance links are not a source offer.
