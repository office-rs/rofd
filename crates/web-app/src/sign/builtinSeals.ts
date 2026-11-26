// The seal pictures the app ships with (spec 2026-11-23 §13): three PNGs
// under `public/seals/`, drawn once with simple shapes. A user may pick a
// picture of their own instead.
//
// The paths carry Vite's base (`/` in dev, `/rofd/` under GitHub Pages).

const BASE = import.meta.env.BASE_URL;

export interface BuiltinSeal {
  name: string;
  url: string;
}

export const BUILTIN_SEALS: BuiltinSeal[] = [
  { name: '圆章', url: `${BASE}seals/round.png` },
  { name: '方章', url: `${BASE}seals/square.png` },
  { name: '条章', url: `${BASE}seals/bar.png` },
];
