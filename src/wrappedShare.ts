import { toPng } from 'html-to-image';

export async function blobToBase64(b: Blob): Promise<string> {
  const buf = await b.arrayBuffer();
  const bytes = new Uint8Array(buf);
  let bin = '';
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}

export async function createWrappedPng(opts: {
  node: HTMLElement;
  width: number;
  height: number;
}): Promise<Blob> {
  // html-to-image returns a data URL; convert to blob.
  const dataUrl = await toPng(opts.node, {
    cacheBust: true,
    width: opts.width,
    height: opts.height,
    pixelRatio: 1,
    backgroundColor: '#0b0b10'
  });
  const res = await fetch(dataUrl);
  return await res.blob();
}
