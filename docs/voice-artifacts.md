# Voice Model Artifacts Catalog

This catalog documents the pinned artifact sources, checksums, and license constraints for all local speech-to-text, keyword spotting (KWS), and voice activity detection (VAD) models supported by Taurine.

## Model Catalog Table

| Model Identifier | Architecture / Engine | Pinned URL | Archive Format | Size (Est.) | SHA-256 Checksum | License | Notes |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `parakeet-tdt-0.6b-v3` | FastConformer + TDT INT8 (`sherpa-onnx`) | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2` | `.tar.bz2` | ~600 MB | Verified on download | CC-BY-4.0 | Requires attribution in settings/about |
| `whisper-large-v3-turbo` | OpenAI Transformer Q5_0 (`whisper-rs`) | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin` | Direct Binary | ~550 MB | Verified on download | MIT | 4-layer decoder, 99+ languages |
| `distil-whisper-large-v3` | Distil-Whisper Q5_0 (`whisper-rs`) | `https://huggingface.co/distil-whisper/distil-large-v3-ggml/resolve/main/ggml-distil-large-v3.bin` | Direct Binary | ~450 MB | Verified on download | MIT | English-optimized, 4-6x faster CPU |
| `whisper-small-en` | OpenAI Small.en Q5_0 (`whisper-rs`) | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en-q5_0.bin` | Direct Binary | ~170 MB | Verified on download | MIT | Balanced laptop CPU engine |
| `whisper-base-en` | OpenAI Base.en Q5_0 (`whisper-rs`) | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en-q5_0.bin` | Direct Binary | ~60 MB | Verified on download | MIT | Ultra-lightweight CPU fallback |
| `moonshine-base-en` | UsefulSensors Base INT8 (`sherpa-onnx`) | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-base-en-int8.tar.bz2` | `.tar.bz2` | ~135 MB | Verified on download | MIT | Variable length encoder |
| `moonshine-tiny-en` | UsefulSensors Tiny INT8 (`sherpa-onnx`) | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-tiny-en-int8.tar.bz2` | `.tar.bz2` | ~27 MB | Verified on download | MIT | Active verifier for trigger spotting |
| `kws-zipformer-zh-en-3M` | Zipformer2 KWS INT8 (`sherpa-onnx`) | `https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20.tar.bz2` | `.tar.bz2` | ~15 MB | Verified on download | Apache-2.0 | Passive background wake spotter |
| `silero_vad_v6` | Silero VAD v6 ONNX (`sherpa-onnx`) | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx` | Direct ONNX | ~2 MB | Verified on download | MIT | VAD gate (32ms frames) |

## Production Mirroring

- **Testing/Development**: Pinned Hugging Face / GitHub release artifact URLs.
- **Production Distribution**: Dedicated Cloudflare R2 bucket (`models.taurine.app`) serving byte-identical binaries with zero egress fees.
- **Integrity**: Every downloaded archive or binary is verified against its SHA-256 hash prior to atomic rename.
