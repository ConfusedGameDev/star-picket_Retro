#!/usr/bin/env python3
"""Synthesize the game's instrument samples into sfx/*.wav (mono, 16-bit, 18157 Hz, the mixer rate).

Pitched instruments are recorded at one note and the game re-pitches them per chord note with the
mixer's playback speed, so a single sample covers every note.
Run from the repo root:  python3 tools/make_sfx.py   (standard library only)
"""
import math
import random
import struct
import wave
from pathlib import Path

RATE = 18157
OUT = Path(__file__).resolve().parent.parent / "sfx"
OUT.mkdir(exist_ok=True)
random.seed(3)
A4, A2 = 440.0, 110.0


def write(name, samples, gain=0.9):
    peak = max(1e-9, max(abs(s) for s in samples))
    with wave.open(str(OUT / name), "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, s / peak * gain)) * 32767)) for s in samples))
    print(f"{name:10} {len(samples) / RATE:.2f}s")


def n(sec): return int(sec * RATE)
def tri(ph): return 4 * abs((ph % 1) - 0.5) - 1
def saw(ph): return 2 * (ph % 1) - 1
def sq(ph): return 1.0 if (ph % 1) < 0.5 else -1.0


def lowpass(xs, cutoff_at):
    """one-pole lowpass whose cutoff (Hz) can change over time"""
    out, y = [], 0.0
    for i, x in enumerate(xs):
        c = cutoff_at(i / RATE)
        a = 1 - math.exp(-2 * math.pi * c / RATE)
        y += a * (x - y)
        out.append(y)
    return out


def env(t, attack, decay):
    return min(1, t / attack) * math.exp(-t / decay) if attack else math.exp(-t / decay)


# pluck: detuned triangle + a little square through a closing filter (the web version's kill note)
L = n(0.45)
raw = [tri(A4 * i / RATE) + 0.35 * sq(A4 * 1.004 * i / RATE) for i in range(L)]
f = lowpass(raw, lambda t: 440 * 1.2 + 440 * 7 * math.exp(-t / 0.08))
write("pluck.wav", [x * env(i / RATE, 0.004, 0.13) for i, x in enumerate(f)])

# short blip for bullet hits and launches
L = n(0.07)
write("blip.wav", [sq(A4 * i / RATE) * env(i / RATE, 0.001, 0.02) for i in range(L)], gain=0.6)

# FM bell for cannon pods
L = n(0.9)
write("bell.wav", [math.sin(2 * math.pi * A4 * t + 2.2 * math.exp(-t / 0.25) * math.sin(2 * math.pi * A4 * 3.5 * t)) * env(t, 0.003, 0.3)
                   for t in (i / RATE for i in range(L))])

# bass: saw + sub through a lowpass
L = n(0.26)
raw = [saw(A2 * i / RATE) + 0.5 * sq(A2 / 2 * i / RATE) for i in range(L)]
f = lowpass(raw, lambda t: 110 * 1.3 + 110 * 4 * math.exp(-t / 0.06))
write("bass.wav", [x * env(i / RATE, 0.003, 0.09) for i, x in enumerate(f)], gain=0.8)

# kick: sine dropping 140 -> 42 Hz
L, ph, out = n(0.28), 0.0, []
for i in range(L):
    t = i / RATE
    ph += (42 + 98 * math.exp(-t / 0.03)) / RATE
    out.append(math.sin(2 * math.pi * ph) * env(t, 0.001, 0.08))
write("kick.wav", out)

noise = [random.uniform(-1, 1) for _ in range(n(1.2))]


def highpass(xs, c):
    a = 1 - math.exp(-2 * math.pi * c / RATE)
    out, y = [], 0.0
    for x in xs:
        y += a * (x - y)
        out.append(x - y)
    return out


write("hat.wav", [x * env(i / RATE, 0, 0.012) for i, x in enumerate(highpass(noise[:n(0.05)], 6000))], gain=0.5)
clap = highpass(lowpass(noise[:n(0.14)], lambda t: 2400), 900)
write("clap.wav", [x * env(i / RATE, 0.002, 0.035) for i, x in enumerate(clap)], gain=0.6)

# explosion crunch: noise with a falling lowpass
write("boom.wav", [x * env(i / RATE, 0.001, 0.12) for i, x in enumerate(lowpass(noise[:n(0.4)], lambda t: 80 + 1500 * math.exp(-t / 0.1)))])

# station: long low rumble with a sub drop
L, ph, out = n(1.1), 0.0, []
rum = lowpass(noise[:L], lambda t: 60 + 900 * math.exp(-t / 0.3))
for i in range(L):
    t = i / RATE
    ph += (35 + 105 * math.exp(-t / 0.25)) / RATE
    out.append(rum[i] * env(t, 0.002, 0.35) + 0.8 * math.sin(2 * math.pi * ph) * env(t, 0.002, 0.4))
write("big.wav", out)

# player death: falling saw over noise
L, ph, out = n(1.0), 0.0, []
for i in range(L):
    t = i / RATE
    ph += (50 + 470 * math.exp(-t / 0.35)) / RATE
    out.append((0.6 * saw(ph) + 0.5 * noise[i]) * env(t, 0.002, 0.35))
write("die.wav", lowpass(out, lambda t: 2500))

# Condition Red siren: a sawtooth sweeping up 520 -> 780 Hz and back down
L, ph, out = n(0.7), 0.0, []
for i in range(L):
    t = i / RATE
    u = t / 0.35 if t < 0.35 else 2 - t / 0.35
    ph += (520 + 260 * u) / RATE
    out.append(saw(ph) * min(1, t / 0.02) * min(1, (0.7 - t) / 0.05))
write("siren.wav", lowpass(out, lambda t: 1800), gain=0.7)
