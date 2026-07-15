"""Loopback-only Parler-TTS Mini provider for Sanymar.

The user starts this process explicitly with an existing local model. It does
not download models, accept reference audio, or log dialogue/descriptions.
"""

from __future__ import annotations

import argparse
import io
import json
import math
import struct
import threading
import time
import wave
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any


HOST = "127.0.0.1"
MODEL_NAME = "parler-tts-mini-v1"
PROVIDER_NAME = "parler_tts_mini"
MAX_REQUEST_BYTES = 16 * 1024
SPEAKERS = ("Jon", "Lea", "Gary", "Jenna", "Mike")
DELIVERY_STYLES = (
    "neutral",
    "warm",
    "energetic",
    "playful",
    "reflective",
    "authoritative",
)


def delivery_description(speaker: str, style: str, rate: float) -> str:
    direction = {
        "neutral": "confident, clear, and natural like an English music radio presenter",
        "warm": "warm and conversational, with relaxed confidence and gentle emphasis",
        "energetic": "bright, upbeat, and animated like a lively music radio host, with confident emphasis and expressive rise and fall, while keeping every phrase smooth and connected rather than shouting or straining",
        "playful": "lightly amused and playful, landing dry humour naturally without exaggeration",
        "reflective": "thoughtful and intimate, with subtle emotional warmth",
        "authoritative": "confident and deliberate like a polished station announcer, without sounding theatrical",
    }[style]
    if style == "energetic":
        pace = "at a lively, slightly quick broadcast pace"
    elif rate >= 1.08:
        pace = "at a slightly quick pace"
    elif rate <= 0.92:
        pace = "at an unhurried pace"
    else:
        pace = "at a moderate pace"
    return (
        f"{speaker}'s voice is {direction}, {pace}. "
        f"{speaker}'s voice is smooth and stable, recorded with very clear close-up audio, "
        "with no vocal distortion, broken words, background noise, or reverberation."
    )


def validate_request(value: Any) -> tuple[str, str, str, float, float]:
    if not isinstance(value, dict) or set(value) != {
        "text",
        "speaker",
        "deliveryStyle",
        "rate",
        "volume",
    }:
        raise ValueError("invalid request fields")
    text = value["text"]
    speaker = value["speaker"]
    style = value["deliveryStyle"]
    rate = value["rate"]
    volume = value["volume"]
    if (
        not isinstance(text, str)
        or not text.strip()
        or len(text) > 4_000
        or "\0" in text
        or speaker not in SPEAKERS
        or style not in DELIVERY_STYLES
        or isinstance(rate, bool)
        or not isinstance(rate, (int, float))
        or not math.isfinite(float(rate))
        or not 0.5 <= float(rate) <= 2.0
        or isinstance(volume, bool)
        or not isinstance(volume, (int, float))
        or not math.isfinite(float(volume))
        or not 0.0 <= float(volume) <= 1.0
    ):
        raise ValueError("invalid request value")
    return text, speaker, style, float(rate), float(volume)


def validate_model_directory(value: Path) -> Path:
    model_directory = value.expanduser().resolve(strict=True)
    if not model_directory.is_dir():
        raise ValueError("model directory is not a directory")
    for name in ("config.json", "tokenizer_config.json", "model.safetensors"):
        if not (model_directory / name).is_file():
            raise ValueError(f"model directory is missing {name}")
    return model_directory


def pcm16_wav(samples: Any, sample_rate: int, volume: float) -> bytes:
    flattened = samples.reshape(-1).tolist()
    pcm = bytearray()
    for raw_sample in flattened:
        sample = float(raw_sample) * volume
        if not math.isfinite(sample):
            raise ValueError("non-finite audio")
        pcm.extend(struct.pack("<h", round(max(-1.0, min(1.0, sample)) * 32767)))
    if not pcm or not 8_000 <= sample_rate <= 192_000:
        raise ValueError("invalid audio")
    output = io.BytesIO()
    with wave.open(output, "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(sample_rate)
        wav.writeframes(pcm)
    return output.getvalue()


class ParlerEngine:
    def __init__(self, model_directory: Path) -> None:
        import torch
        from parler_tts import ParlerTTSForConditionalGeneration
        from transformers import AutoTokenizer

        if not torch.cuda.is_available():
            raise RuntimeError("CUDA is unavailable")
        self._torch = torch
        self._tokenizer = AutoTokenizer.from_pretrained(
            model_directory, local_files_only=True
        )
        self._model = ParlerTTSForConditionalGeneration.from_pretrained(
            model_directory,
            local_files_only=True,
            attn_implementation="eager",
            torch_dtype=torch.float16,
        ).to("cuda:0")
        self._model.eval()
        self.sample_rate = int(self._model.audio_encoder.config.sampling_rate)
        self._lock = threading.Lock()

    def synthesize(
        self, text: str, speaker: str, style: str, rate: float, volume: float
    ) -> bytes:
        description = delivery_description(speaker, style, rate)
        with self._lock:
            direction = self._tokenizer(description, return_tensors="pt").to("cuda:0")
            dialogue = self._tokenizer(text, return_tensors="pt").to("cuda:0")
            with self._torch.inference_mode():
                generated = self._model.generate(
                    input_ids=direction.input_ids,
                    attention_mask=direction.attention_mask,
                    prompt_input_ids=dialogue.input_ids,
                    prompt_attention_mask=dialogue.attention_mask,
                    do_sample=True,
                    temperature=0.8,
                )
            samples = generated.detach().float().cpu().numpy().squeeze()
        return pcm16_wav(samples, self.sample_rate, volume)


class ProviderServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address: tuple[str, int], engine: ParlerEngine) -> None:
        super().__init__(address, ProviderHandler)
        self.engine = engine


class ProviderHandler(BaseHTTPRequestHandler):
    server: ProviderServer

    def log_message(self, _format: str, *args: object) -> None:
        return

    def do_GET(self) -> None:
        if self.path != "/health":
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        self._send_json(
            HTTPStatus.OK,
            {
                "ready": True,
                "provider": PROVIDER_NAME,
                "model": MODEL_NAME,
                "sampleRate": self.server.engine.sample_rate,
                "speakers": list(SPEAKERS),
            },
        )

    def do_POST(self) -> None:
        if self.path != "/synthesize":
            self.send_error(HTTPStatus.NOT_FOUND)
            return
        try:
            content_length = int(self.headers.get("Content-Length", "0"))
            if not 0 < content_length <= MAX_REQUEST_BYTES:
                raise ValueError("invalid content length")
            if self.headers.get_content_type() != "application/json":
                raise ValueError("invalid content type")
            body = self.rfile.read(content_length)
            request = json.loads(body.decode("utf-8"))
            text, speaker, style, rate, volume = validate_request(request)
        except (UnicodeDecodeError, json.JSONDecodeError, TypeError, ValueError):
            self._send_json(HTTPStatus.BAD_REQUEST, {"error": "invalid_request"})
            return
        try:
            audio = self.server.engine.synthesize(text, speaker, style, rate, volume)
        except Exception:
            self._send_json(
                HTTPStatus.SERVICE_UNAVAILABLE, {"error": "synthesis_failed"}
            )
            return
        try:
            self.send_response(HTTPStatus.OK)
            self.send_header("Content-Type", "audio/wav")
            self.send_header("Content-Length", str(len(audio)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(audio)
        except (BrokenPipeError, ConnectionResetError):
            return

    def _send_json(self, status: HTTPStatus, value: dict[str, object]) -> None:
        body = json.dumps(value, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", type=Path, required=True)
    parser.add_argument("--port", type=int, default=43822)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not 1 <= args.port <= 65_535:
        raise SystemExit("port must be between 1 and 65535")
    model_directory = validate_model_directory(args.model_dir)
    print("Loading user-managed Parler Mini model...")
    started = time.perf_counter()
    engine = ParlerEngine(model_directory)
    print(
        f"Parler Mini ready on http://{HOST}:{args.port} "
        f"after {time.perf_counter() - started:.1f}s"
    )
    server = ProviderServer((HOST, args.port), engine)
    try:
        server.serve_forever(poll_interval=0.25)
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
