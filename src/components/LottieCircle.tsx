import { useEffect, useMemo, useRef } from 'react';
import lottie, { type AnimationItem } from 'lottie-web';

// Simple in-memory cache to avoid repeatedly downloading/parsing large JSON.
const LOTTIE_JSON_CACHE: Map<string, any> = new Map();

type Props = {
  /** public path like `/base3-idle.json` */
  src: string;
  /** Optional prioritized fallback list; first that loads is used. */
  srcList?: string[];
  size: number;
  loop?: boolean;
  /** When false, the animation is stopped to reduce CPU usage. */
  playing?: boolean;
};

export function LottieCircle({ src, srcList, size, loop = true, playing = true }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const animRef = useRef<AnimationItem | null>(null);

  const style = useMemo(
    () => ({
      width: size,
      height: size,
      borderRadius: '9999px',
      overflow: 'hidden',
      // Keep a perfectly clean circle with no background tint
      background: 'transparent'
    }),
    [size]
  );

  useEffect(() => {
    let cancelled = false;

    const controller = new AbortController();

    async function load() {
      if (!containerRef.current) return;

      // Cleanup previous
      if (animRef.current) {
        animRef.current.destroy();
        animRef.current = null;
      }

      // Fetch the JSON ourselves so we can cache and handle errors cleanly, with fallbacks.
      // NOTE: these JSON files can be large; caching avoids repeat parsing.
      const candidates = (srcList && srcList.length ? srcList : [src]).filter(Boolean);
      let animationData: any | null = null;
      for (const cand of candidates) {
        const cached = LOTTIE_JSON_CACHE.get(cand);
        if (cached) {
          animationData = cached;
          break;
        }
        try {
          const res = await fetch(cand, { cache: 'force-cache', signal: controller.signal });
          if (!res.ok) throw new Error(`HTTP ${res.status}`);
          const data = await res.json();
          LOTTIE_JSON_CACHE.set(cand, data);
          animationData = data;
          break;
        } catch {
          // Try next candidate
        }
      }
      if (!animationData) throw new Error(`Failed to load lottie from candidates: ${candidates.join(', ')}`);
      if (cancelled) return;

      // Cap DPR for canvas rendering to avoid 2x+ work on Retina/HiDPI.
      const dpr = Math.min((window.devicePixelRatio || 1), 1.25);

      animRef.current = lottie.loadAnimation({
        container: containerRef.current,
        // Canvas is generally cheaper than SVG for continuously running animations.
        renderer: 'canvas',
        loop,
        // Don't autoplay when not playing to avoid starting and immediately stopping.
        autoplay: Boolean(playing),
        animationData,
        rendererSettings: {
          clearCanvas: true,
          progressiveLoad: true,
          // Lower device pixel ratio reduces fill-rate work on the GPU/CPU.
          dpr
        }
      });

      // Reduce work: avoid subframe interpolation.
      try {
        animRef.current.setSubframe(false);
      } catch {
        // ignore
      }

      // No need to stop here; `autoplay` already reflects `playing`.
    }

    load().catch((err) => {
      // eslint-disable-next-line no-console
      console.error('[LottieCircle] load error', err);
    });

    return () => {
      cancelled = true;
      try {
        controller.abort();
      } catch {
        // ignore
      }
      if (animRef.current) {
        animRef.current.destroy();
        animRef.current = null;
      }
    };
  }, [src, srcList && srcList.join('|')]);

  // Reflect loop changes without destroying/recreating the animation.
  useEffect(() => {
    if (!animRef.current) return;
    try {
      // lottie-web supports toggling loop flag at runtime.
      (animRef.current as any).loop = loop;
    } catch {
      // ignore
    }
  }, [loop]);

  useEffect(() => {
    const a = animRef.current;
    if (!a) return;
    try {
      if (playing) a.play();
      else a.stop();
    } catch {
      // ignore
    }
  }, [playing]);

  // Pause all animations when the document is hidden to save CPU.
  useEffect(() => {
    const onVis = () => {
      const a = animRef.current;
      if (!a) return;
      try {
        if (document.hidden) a.pause();
        else if (playing) a.play();
      } catch {
        // ignore
      }
    };
    document.addEventListener('visibilitychange', onVis);
    return () => document.removeEventListener('visibilitychange', onVis);
  }, [playing]);

  return <div ref={containerRef} style={style} />;
}
