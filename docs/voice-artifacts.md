# Voice Model Artifacts Catalog

This catalog documents the pinned artifact sources, checksums, and license constraints for all local speech-to-text models supported by Taurine.

Free-tier local dictation is English-only; multilingual dictation is reserved for Pro cloud.

## Model Catalog Table

| Model Identifier | Alias | Architecture / Engine | Pinned URL | Archive Format | Size (Est.) | SHA-256 Checksum | License | Notes |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `parakeet-unified-en-0.6b` | `best` | FastConformer RNN-T INT8 (`sherpa-onnx`) | `https://huggingface.co/csukuangfj2/sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming/resolve/main/` | Multi-file / `.tar.bz2` fallback | ~631 MB | Verified on download | CC-BY-4.0 | Best quality English dictation; requires 8 GB+ RAM default |
| `parakeet-tdt-0.6b-v2` | `balanced` | FastConformer TDT INT8 (`sherpa-onnx`) | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2` | `.tar.bz2` | ~460 MB | Verified on download | CC-BY-4.0 | Balanced English dictation with duration-skipping |
| `parakeet-tdt-ctc-110m` | `fast` | Hybrid FastConformer TDT-CTC INT8 (`sherpa-onnx`) | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet_tdt_ctc_110m-en-36000-int8.tar.bz2` | `.tar.bz2` | ~135 MB | Verified on download | CC-BY-4.0 | Light and fast English dictation for constrained machines |

## Selecting a Model

`auto` (default) picks `best` on machines with 16 GiB+ RAM, otherwise `fast`.
Pin a tier explicitly; aliases canonicalize to the full identifier on save:

```bash
taurine config set voice_model auto
taurine config set voice_model best
taurine config set voice_model balanced
taurine config set voice_model fast
```

## Production Mirroring

- **Testing/Development**: Pinned Hugging Face / GitHub release artifact URLs.
- **Production Distribution**: Dedicated Cloudflare R2 bucket (`models.taurine.app`) serving byte-identical binaries with zero egress fees.
- **Integrity**: Every downloaded archive or binary is verified against its SHA-256 hash prior to atomic rename.
