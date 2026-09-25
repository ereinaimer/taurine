# Voice Model Artifacts Catalog

This catalog documents the pinned artifact sources, checksums, and license constraints for all local speech-to-text, keyword spotting (KWS), and voice activity detection (VAD) models supported by Taurine.

Free-tier local dictation is English-only; multilingual dictation is reserved for Pro cloud.

## Model Catalog Table

| Model Identifier | Architecture / Engine | Pinned URL | Archive Format | Size (Est.) | SHA-256 Checksum | License | Notes |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `parakeet-unified-en-0.6b` | FastConformer RNN-T INT8 (`sherpa-onnx`) | `https://huggingface.co/csukuangfj2/sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming/resolve/main/` | Multi-file / `.tar.bz2` fallback | ~631 MB | Verified on download | CC-BY-4.0 | Best quality English dictation; requires 8 GB+ RAM default |
| `parakeet-tdt-ctc-110m` | Hybrid FastConformer TDT-CTC INT8 (`sherpa-onnx`) | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet_tdt_ctc_110m-en-36000-int8.tar.bz2` | `.tar.bz2` | ~135 MB | Verified on download | CC-BY-4.0 | Light & fast English dictation; constrained machine default |
| `kws-zipformer-zh-en-3M` | Zipformer2 KWS INT8 (`sherpa-onnx`) | `https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20.tar.bz2` | `.tar.bz2` | ~15 MB | Verified on download | Apache-2.0 | Passive background wake spotter |
| `silero_vad_v6` | Silero VAD v6 ONNX (`sherpa-onnx`) | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx` | Direct ONNX | ~2 MB | Verified on download | MIT | VAD gate (32ms frames) |

## Production Mirroring

- **Testing/Development**: Pinned Hugging Face / GitHub release artifact URLs.
- **Production Distribution**: Dedicated Cloudflare R2 bucket (`models.taurine.app`) serving byte-identical binaries with zero egress fees.
- **Integrity**: Every downloaded archive or binary is verified against its SHA-256 hash prior to atomic rename.
