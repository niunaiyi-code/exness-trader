export type TradeSound = 'open' | 'close' | 'stop_loss' | 'lock' | 'alert';

/** MT5 原生音效映射表 */
const soundFileMap: Record<TradeSound, string> = {
  open: '/sounds/expert.wav',      // MT5 SOUND_ORDER: EA 开仓/加仓确认音
  close: '/sounds/ok.wav',         // MT5 SOUND_TP: 经典止盈落袋/成功音
  stop_loss: '/sounds/stops.wav',  // MT5 止损离场警示音
  lock: '/sounds/alert.wav',       // MT5 单边对冲全锁仓风控警报音（明确区分于止损）
  alert: '/sounds/alert.wav',      // MT5 异常风险/警报音
};

let audioContext: AudioContext | null = null;
const audioBufferCache: Partial<Record<TradeSound, AudioBuffer>> = {};
let isPreloading = false;

function getAudioContext(): AudioContext | null {
  const Context =
    window.AudioContext ||
    (window as typeof window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Context) {
    console.warn('[tradeSounds] Web Audio API is not supported in this browser.');
    return null;
  }
  audioContext ??= new Context();
  if (audioContext.state === 'suspended') {
    void audioContext.resume();
  }
  return audioContext;
}

/**
 * 并行预加载所有 MT5 原版 .wav 音频到内存 AudioBuffer 中
 */
async function preloadAudioBuffers(): Promise<void> {
  if (isPreloading) return;
  isPreloading = true;
  const ctx = getAudioContext();
  if (!ctx) {
    isPreloading = false;
    return;
  }

  const entries = Object.entries(soundFileMap) as [TradeSound, string][];
  await Promise.all(
    entries.map(async ([key, url]) => {
      try {
        const resp = await fetch(url);
        if (!resp.ok) {
          console.warn(`[tradeSounds] HTTP ${resp.status} fetching ${url}`);
          return;
        }
        const arrayBuffer = await resp.arrayBuffer();
        const decoded = await ctx.decodeAudioData(arrayBuffer);
        audioBufferCache[key] = decoded;
      } catch (err) {
        console.warn(`[tradeSounds] Failed to load/decode audio for ${key} (${url}):`, err);
      }
    })
  );
  isPreloading = false;
}

// 监听用户首次交互（点击或按键），解锁浏览器的音频自动播放限制并预加载
if (typeof window !== 'undefined') {
  const unlockAudio = () => {
    const ctx = getAudioContext();
    if (ctx && ctx.state === 'suspended') {
      void ctx.resume();
    }
    void preloadAudioBuffers();
    window.removeEventListener('click', unlockAudio);
    window.removeEventListener('keydown', unlockAudio);
    window.removeEventListener('touchstart', unlockAudio);
  };
  window.addEventListener('click', unlockAudio, { passive: true });
  window.addEventListener('keydown', unlockAudio, { passive: true });
  window.addEventListener('touchstart', unlockAudio, { passive: true });
}

/**
 * 播放指定交易事件的 MT5 原版音效（单链路，直接播放内存 AudioBuffer）
 */
export function playTradeSound(event: TradeSound): void {
  const ctx = getAudioContext();
  if (!ctx) return;

  const buffer = audioBufferCache[event];
  if (!buffer) {
    void preloadAudioBuffers().then(() => {
      const b = audioBufferCache[event];
      if (b && ctx) {
        try {
          const source = ctx.createBufferSource();
          source.buffer = b;
          source.connect(ctx.destination);
          source.start(0);
        } catch (err) {
          console.warn(`[tradeSounds] Error playing delayed ${event} sound:`, err);
        }
      }
    });
    return;
  }

  try {
    const source = ctx.createBufferSource();
    const gainNode = ctx.createGain();
    gainNode.gain.value = 0.6; // 舒适音量
    source.buffer = buffer;
    source.connect(gainNode);
    gainNode.connect(ctx.destination);
    source.start(0);
  } catch (err) {
    console.warn(`[tradeSounds] Error playing ${event} sound:`, err);
  }
}
