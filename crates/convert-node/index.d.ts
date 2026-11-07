/** Per-call conversion options; every field optional. */
export interface ConvertOptions {
  /** Watermark layer; omit = kernel default (on). */
  watermark?: boolean;
  /** Annotation layer; omit = kernel default (on). */
  annotations?: boolean;
  /** Seal layer; omit = kernel default (on). */
  seal?: boolean;
  /** Page budget; omit / <= 0 = kernel default (10000). */
  maxPages?: number;
  /** Media byte budget; omit / <= 0 = kernel default (1 GiB). */
  maxMediaBytes?: number;
}

/** One conversion's result. */
export interface ConvertOutput {
  bytes: Buffer;
  warnings: string[];
}

export declare class Converter {
  constructor();
  /**
   * Convert OFD bytes to PDF bytes. Kernel panics surface as JS errors
   * (panics never abort the process).
   */
  convertToPdf(ofd: Buffer, options?: ConvertOptions): ConvertOutput;
}

/** Register font bytes into the process-level registry (append-only, process-wide, idempotent). Unparsable -> false. */
export declare function registerFont(bytes: Buffer): boolean;
/** Register a font file; I/O failure throws, unparsable font -> false. */
export declare function registerFontFile(path: string): boolean;
/** Anchor the process default font (kernel D19); a later anchor wins. */
export declare function registerDefaultFont(bytes: Buffer): boolean;
/** Whether a default font is anchored in the process registry. */
export declare function hasDefaultFont(): boolean;
/** Font families visible to the process registry (registrations only). */
export declare function registeredFamilies(): string[];
/** Package version (kept in lockstep across all channels). */
export declare function version(): string;

/** Slot pool over worker_threads (one Converter per worker, spec D11). */
export interface Pool {
  convertToPdf(ofd: Buffer, options?: ConvertOptions): Promise<ConvertOutput>;
  close(): Promise<void>;
  readonly size: number;
}
export declare function createPool(size?: number): Pool;
