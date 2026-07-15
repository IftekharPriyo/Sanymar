"""Offline Parler-TTS Mini voice-quality probe for Sanymar.

This tool deliberately requires an existing local model directory. It never
downloads weights, accepts reference audio, or contacts a model service.
"""

from __future__ import annotations

import argparse
import json
import math
import struct
import sys
import time
import wave
from dataclasses import asdict, dataclass
from pathlib import Path


MODEL_FILES = ("config.json", "tokenizer_config.json")
SPEAKERS = ("Jon", "Lea", "Gary", "Jenna", "Mike")


@dataclass(frozen=True)
class ProbeCase:
    name: str
    dialogue: str
    description: str


@dataclass(frozen=True)
class ProbeResult:
    case: str
    output_file: str
    audio_seconds: float
    synthesis_seconds: float
    real_time_factor: float
    peak_gpu_memory_mib: float


def cases_for(speaker: str) -> tuple[ProbeCase, ...]:
    clean = (
        f"{speaker}'s voice is recorded with very clear close-up audio, "
        "with no background noise and no reverberation."
    )
    return (
        ProbeCase(
            "neutral",
            "You're listening to Sanymar. Good music, good company, and no unnecessary fuss.",
            f"{speaker} speaks like a confident English music radio presenter, clearly and naturally at a moderate pace. {clean}",
        ),
        ProbeCase(
            "warm",
            "That one leaves a little glow behind. Let's keep the evening moving.",
            f"{speaker} speaks warmly and conversationally, with relaxed confidence, gentle emphasis, and a moderate pace. {clean}",
        ),
        ProbeCase(
            "energetic",
            "Turn it up a touch. The next track arrives with enough momentum to rearrange the room.",
            f"{speaker} sounds upbeat and animated like a lively music radio presenter, with rhythmic momentum and restrained excitement. {clean}",
        ),
        ProbeCase(
            "playful",
            "That bassline walked in like it had already reserved the best seat.",
            f"{speaker} sounds lightly amused and playful, landing the dry humour naturally without exaggerating it. {clean}",
        ),
        ProbeCase(
            "reflective",
            "Some songs don't demand attention. They simply wait until the room becomes quiet enough.",
            f"{speaker} speaks thoughtfully and intimately, with an unhurried reflective delivery and subtle emotional warmth. {clean}",
        ),
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", type=Path, required=True)
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path(".local/parler-probe"),
    )
    parser.add_argument("--speaker", choices=SPEAKERS, default="Jon")
    parser.add_argument("--case", choices=("all", "neutral", "warm", "energetic", "playful", "reflective"), default="all")
    parser.add_argument("--seed", type=int, default=7)
    return parser.parse_args()


def validate_model_directory(model_directory: Path) -> Path:
    resolved = model_directory.expanduser().resolve(strict=True)
    if not resolved.is_dir():
        raise ValueError("model directory is not a directory")
    missing = [name for name in MODEL_FILES if not (resolved / name).is_file()]
    if missing:
        raise ValueError(f"model directory is missing: {', '.join(missing)}")
    if not any(resolved.glob("*.safetensors")):
        raise ValueError("model directory contains no .safetensors weights")
    return resolved


def write_pcm16_wav(path: Path, samples: object, sample_rate: int) -> None:
    flattened = samples.reshape(-1).tolist()
    pcm = bytearray()
    for raw_sample in flattened:
        sample = float(raw_sample)
        if not math.isfinite(sample):
            raise ValueError("model returned a non-finite audio sample")
        pcm.extend(struct.pack("<h", round(max(-1.0, min(1.0, sample)) * 32767)))
    if not pcm:
        raise ValueError("model returned no audio samples")
    temporary = path.with_suffix(".tmp")
    with wave.open(str(temporary), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(sample_rate)
        output.writeframes(pcm)
    temporary.replace(path)


def main() -> int:
    args = parse_args()
    try:
        model_directory = validate_model_directory(args.model_dir)
    except (OSError, ValueError) as error:
        print(f"Model validation failed: {error}", file=sys.stderr)
        return 2

    try:
        import torch
        from parler_tts import ParlerTTSForConditionalGeneration
        from transformers import AutoTokenizer, set_seed
    except ImportError as error:
        print(f"Runtime dependency is missing: {error}", file=sys.stderr)
        return 3

    if not torch.cuda.is_available():
        print("CUDA is unavailable; this probe intentionally refuses a slow CPU run.", file=sys.stderr)
        return 4

    args.output_dir.mkdir(parents=True, exist_ok=True)
    output_directory = args.output_dir.resolve(strict=True)
    selected = tuple(
        case for case in cases_for(args.speaker) if args.case == "all" or case.name == args.case
    )

    dtype = torch.float16
    print(f"Loading local model on {torch.cuda.get_device_name(0)}...")
    load_started = time.perf_counter()
    tokenizer = AutoTokenizer.from_pretrained(model_directory, local_files_only=True)
    model = ParlerTTSForConditionalGeneration.from_pretrained(
        model_directory,
        local_files_only=True,
        attn_implementation="eager",
        torch_dtype=dtype,
    ).to("cuda:0")
    model.eval()
    sample_rate = int(model.audio_encoder.config.sampling_rate)
    print(f"Model loaded in {time.perf_counter() - load_started:.2f}s at {sample_rate} Hz.")

    results: list[ProbeResult] = []
    for index, case in enumerate(selected):
        set_seed(args.seed + index)
        torch.cuda.reset_peak_memory_stats()
        description = tokenizer(case.description, return_tensors="pt").to("cuda:0")
        dialogue = tokenizer(case.dialogue, return_tensors="pt").to("cuda:0")
        started = time.perf_counter()
        with torch.inference_mode():
            generated = model.generate(
                input_ids=description.input_ids,
                attention_mask=description.attention_mask,
                prompt_input_ids=dialogue.input_ids,
                prompt_attention_mask=dialogue.attention_mask,
                do_sample=True,
                temperature=1.0,
            )
        torch.cuda.synchronize()
        synthesis_seconds = time.perf_counter() - started
        samples = generated.detach().float().cpu().numpy().squeeze()
        audio_seconds = float(samples.size) / sample_rate
        if audio_seconds <= 0:
            raise ValueError("model returned an empty audio artifact")
        output_path = output_directory / f"{index + 1:02d}-{args.speaker.lower()}-{case.name}.wav"
        write_pcm16_wav(output_path, samples, sample_rate)
        result = ProbeResult(
            case=case.name,
            output_file=str(output_path),
            audio_seconds=round(audio_seconds, 3),
            synthesis_seconds=round(synthesis_seconds, 3),
            real_time_factor=round(synthesis_seconds / audio_seconds, 3),
            peak_gpu_memory_mib=round(torch.cuda.max_memory_allocated() / 1024 / 1024, 1),
        )
        results.append(result)
        print(
            f"{case.name}: {result.audio_seconds:.2f}s audio in "
            f"{result.synthesis_seconds:.2f}s (RTF {result.real_time_factor:.2f})"
        )

    report_path = output_directory / f"report-{args.speaker.lower()}.json"
    report_path.write_text(
        json.dumps(
            {
                "speaker": args.speaker,
                "seed": args.seed,
                "device": torch.cuda.get_device_name(0),
                "results": [asdict(result) for result in results],
            },
            indent=2,
        ),
        encoding="utf-8",
    )
    print(f"Report: {report_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
