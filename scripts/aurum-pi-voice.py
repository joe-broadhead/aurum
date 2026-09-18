#!/usr/bin/env python3
"""Bridge Aurum `--stdio` JSONL to Pi RPC (`pi --mode rpc`).

Aurum is ears/mouth. Pi is the brain (tools, MCPs, session).

Usage (from an Aurum checkout, headphones on):

  python3 scripts/aurum-pi-voice.py -- --mic --model tiny-q5_1

  # Remote STT/TTS:
  python3 scripts/aurum-pi-voice.py -- --mic --provider openai \\
      --model gpt-4o-mini-transcribe --tts-provider openai --tts-model tts-1 --voice alloy

Env:
  AURUM_BIN   default: `aurum` on PATH, else `cargo run -p aurum-stt --`
  PI_BIN      default: `pi`
  Extra args after `--` go to `aurum converse`.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import threading
from typing import IO, Any

PROTO_V = 1


def take_speakable(buf: list[str]) -> str | None:
    """Port of aurum take_speakable: flush on .!? or long chunk."""
    s = "".join(buf)
    if len(s) < 12:
        return None
    for i, ch in enumerate(s):
        if ch in ".!?" and i + 1 >= 12:
            head, tail = s[: i + 1].strip(), s[i + 1 :].lstrip()
            buf.clear()
            if tail:
                buf.append(tail)
            return head or None
    if len(s) >= 90:
        cut = max(s.rfind(" ", 0, 90), s.rfind(",", 0, 90))
        if cut >= 12:
            head, tail = s[: cut + 1].strip(), s[cut + 1 :].lstrip()
            buf.clear()
            if tail:
                buf.append(tail)
            return head or None
    return None


def aurum_cmd(extra: list[str]) -> list[str]:
    explicit = os.environ.get("AURUM_BIN")
    if explicit:
        return explicit.split() + ["converse", "--stdio", *extra]
    if shutil.which("aurum"):
        return ["aurum", "converse", "--stdio", *extra]
    return ["cargo", "run", "-q", "-p", "aurum-stt", "--", "converse", "--stdio", *extra]


def pi_cmd() -> list[str]:
    return os.environ.get("PI_BIN", "pi").split() + ["--mode", "rpc"]


def write_json(fp: IO[bytes], obj: dict[str, Any]) -> None:
    fp.write((json.dumps(obj, ensure_ascii=False) + "\n").encode("utf-8"))
    fp.flush()


def main() -> int:
    extra = sys.argv[1:]
    if extra[:1] == ["--"]:
        extra = extra[1:]
    if "--stdio" in extra:
        extra = [a for a in extra if a != "--stdio"]

    sys.stderr.write("aurum-pi-voice: starting Aurum sidecar + pi --mode rpc\n")
    aurum = subprocess.Popen(
        aurum_cmd(extra),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=sys.stderr,
    )
    pi = subprocess.Popen(
        pi_cmd(),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=sys.stderr,
    )
    assert aurum.stdin and aurum.stdout and pi.stdin and pi.stdout

    speak_buf: list[str] = []
    buf_lock = threading.Lock()
    aurum_in_lock = threading.Lock()
    pi_in_lock = threading.Lock()
    agent_busy = threading.Event()

    def write_aurum(obj: dict[str, Any]) -> None:
        with aurum_in_lock:
            try:
                write_json(aurum.stdin, obj)
            except BrokenPipeError:
                pass

    def write_pi(obj: dict[str, Any]) -> None:
        with pi_in_lock:
            try:
                write_json(pi.stdin, obj)
            except BrokenPipeError:
                pass

    def flush_speak(end_turn: bool) -> None:
        with buf_lock:
            rest = "".join(speak_buf).strip()
            speak_buf.clear()
        if rest:
            write_aurum({"v": PROTO_V, "type": "speak", "text": rest})
        if end_turn:
            write_aurum({"v": PROTO_V, "type": "end_turn"})

    def from_aurum() -> None:
        for raw in aurum.stdout:
            try:
                ev = json.loads(raw.decode("utf-8", errors="replace"))
            except json.JSONDecodeError:
                continue
            typ = ev.get("type")
            if typ == "user_final":
                text = (ev.get("text") or "").strip()
                if not text:
                    continue
                sys.stderr.write(f"you: {text}\n")
                cmd = "steer" if agent_busy.is_set() else "prompt"
                write_pi({"type": cmd, "message": text})
            elif typ == "error":
                sys.stderr.write(f"aurum error: {ev.get('message')}\n")
            elif typ == "shutdown":
                break

    def from_pi() -> None:
        for raw in pi.stdout:
            try:
                ev = json.loads(raw.decode("utf-8", errors="replace"))
            except json.JSONDecodeError:
                continue
            typ = ev.get("type")
            if typ == "agent_start":
                agent_busy.set()
            elif typ == "agent_settled":
                flush_speak(end_turn=True)
                agent_busy.clear()
            elif typ == "message_update":
                delta = ev.get("assistantMessageEvent") or {}
                if delta.get("type") == "text_delta":
                    piece = delta.get("delta") or ""
                    if not piece:
                        continue
                    with buf_lock:
                        speak_buf.append(piece)
                        flushed: list[str] = []
                        while True:
                            s = take_speakable(speak_buf)
                            if not s:
                                break
                            flushed.append(s)
                    for s in flushed:
                        write_aurum({"v": PROTO_V, "type": "speak", "text": s})

    ta = threading.Thread(target=from_aurum, daemon=True)
    tp = threading.Thread(target=from_pi, daemon=True)
    ta.start()
    tp.start()
    try:
        ta.join()
    except KeyboardInterrupt:
        pass
    write_aurum({"v": PROTO_V, "type": "shutdown"})
    write_pi({"type": "abort"})
    aurum.terminate()
    pi.terminate()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
