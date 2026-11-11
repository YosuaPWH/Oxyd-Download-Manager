// The tone played when a download finishes: two short notes made with Web Audio, so the
// installer carries no sound files.

let ctx: AudioContext | undefined;

export function playDone() {
  try {
    ctx ??= new AudioContext();
    const t0 = ctx.currentTime;
    // A rising fifth: D5, then A5.
    [587.33, 880].forEach((freq, i) => {
      const osc = ctx!.createOscillator();
      const gain = ctx!.createGain();
      const at = t0 + i * 0.11;
      osc.type = "sine";
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0, at);
      gain.gain.linearRampToValueAtTime(0.16, at + 0.012);
      gain.gain.exponentialRampToValueAtTime(0.0001, at + 0.5);
      osc.connect(gain).connect(ctx!.destination);
      osc.start(at);
      osc.stop(at + 0.52);
    });
  } catch {
    // No audio device: a finished download must never fail because of its sound.
  }
}
