#!/usr/bin/env python3
"""Converts RubaiSTT to the reference F32 GGUF used by transcribe.cpp.

The upstream converter needs two things this checkpoint does not ship: a `tokenizer.json`
and a display name for its variant. Both are supplied here, so transcribe.cpp stays unpatched.

Usage: convert-model.py <transcribe.cpp checkout> <model directory> <output.gguf>
Run it inside the Python environment of `scripts/envs/whisper` in the transcribe.cpp checkout.
"""

import importlib.util
import sys
from pathlib import Path

VARIANT = "rubaistt_v2_medium"
DISPLAY_NAME = "RubaiSTT v2 Medium (Uzbek)"
REPO_ID = "islomov/rubaistt_v2_medium"


def write_tokenizer_json(model_dir: Path) -> None:
    if (model_dir / "tokenizer.json").exists():
        return
    from transformers import WhisperTokenizerFast

    tokenizer = WhisperTokenizerFast.from_pretrained(str(model_dir), language="uz", task="transcribe")
    tokenizer.backend_tokenizer.save(str(model_dir / "tokenizer.json"))


def load_converter(transcribe_cpp: Path):
    script = transcribe_cpp / "scripts" / "convert-whisper.py"
    sys.path.insert(0, str(script.parent))
    spec = importlib.util.spec_from_file_location("convert_whisper", script)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    transcribe_cpp, model_dir, output = sys.argv[1:4]
    model_dir = Path(model_dir)
    write_tokenizer_json(model_dir)
    converter = load_converter(Path(transcribe_cpp))
    converter.VARIANT_DISPLAY_NAMES[VARIANT] = DISPLAY_NAME
    return converter.main(
        ["convert-whisper", str(model_dir), output, "--repo-id", REPO_ID, "--variant", VARIANT]
    )


if __name__ == "__main__":
    raise SystemExit(main())
