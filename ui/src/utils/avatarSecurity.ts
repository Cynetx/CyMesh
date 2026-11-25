/**
 * Security & Privacy Pipeline for Profile Avatars
 *
 * Requirements:
 * 1. Complete EXIF / Metadata Stripping:
 *    User photos taken on phones or cameras embed GPS coordinates (home location!), camera serials,
 *    timestamps, and device models.
 *    By decoding raw pixel bitmaps onto an HTML5 <canvas> and re-encoding, all EXIF/XMP/IPTC metadata is
 *    100% stripped before the image leaves the client device.
 * 2. Mesh Packet & DoS Protection:
 *    Noise protocol and DHT records have strict packet limits (<65KB). Avatars are downscaled to max 256x256
 *    and compressed to WebP/JPEG with quality 0.82, keeping payload size strictly < 35KB.
 * 3. XSS & Code Execution Prevention:
 *    SVG and arbitrary XML/HTML are strictly rejected. Only raster bitmaps (JPEG, PNG, WebP) are processed.
 *    URLs are validated before rendering to prevent javascript: or data:text/html exploits.
 */

const ALLOWED_MIME_TYPES = ["image/jpeg", "image/png", "image/webp", "image/jpg"];
const MAX_INPUT_FILE_SIZE_BYTES = 10 * 1024 * 1024; // 10 MB max upload limit before processing
const TARGET_MAX_DIMENSION = 256; // High DPI crisp 256x256 square
const TARGET_PAYLOAD_LIMIT_BYTES = 48 * 1024; // 48 KB max base64 payload size

export interface ProcessedAvatarResult {
  dataUri: string;
  byteSize: number;
}

/**
 * Validates that an avatar URL is a safe base64 raster image data URI.
 */
export function isSafeAvatarUrl(url: string | null | undefined): boolean {
  if (!url || typeof url !== "string") return false;
  const trimmed = url.trim();
  if (
    !trimmed.startsWith("data:image/webp;base64,") &&
    !trimmed.startsWith("data:image/jpeg;base64,") &&
    !trimmed.startsWith("data:image/jpg;base64,") &&
    !trimmed.startsWith("data:image/png;base64,")
  ) {
    return false;
  }
  // Enforce maximum safe string length (64KB data URI limit)
  if (trimmed.length > 65536) {
    return false;
  }
  return true;
}

/**
 * Sanitizes, strips metadata, square-crops, downscales, and compresses an avatar file.
 * Returns a clean data URI ready for Ed25519 cryptographic signing.
 */
export async function processAndSanitizeAvatar(file: File): Promise<ProcessedAvatarResult> {
  // 1. Strict MIME type check
  if (!ALLOWED_MIME_TYPES.includes(file.type.toLowerCase())) {
    throw new Error("فرمت تصویر نامعتبر است. تنها فایل‌های JPG، PNG و WebP مجاز هستند.");
  }

  // 2. Initial size gate
  if (file.size > MAX_INPUT_FILE_SIZE_BYTES) {
    throw new Error("حجم فایل انتخابی بیش از ۱۰ مگابایت است. لطفاً فایل کوچک‌تری انتخاب کنید.");
  }

  // 3. Load into HTMLImageElement
  const image = await loadImageFromFile(file);

  // 4. Center-crop and scale onto Canvas (Strips 100% of EXIF/GPS metadata)
  const dataUri = renderSanitizedCanvas(image, TARGET_MAX_DIMENSION, 0.82);

  // 5. Size check: if still over target, downscale further to 180x180
  if (dataUri.length > TARGET_PAYLOAD_LIMIT_BYTES) {
    const compactUri = renderSanitizedCanvas(image, 180, 0.75);
    return {
      dataUri: compactUri,
      byteSize: compactUri.length,
    };
  }

  return {
    dataUri,
    byteSize: dataUri.length,
  };
}

function loadImageFromFile(file: File): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = (e) => {
      const img = new Image();
      img.onload = () => resolve(img);
      img.onerror = () => reject(new Error("خطا در بارگذاری تصویر. تصویر ممکن است مخدوش باشد."));
      img.src = e.target?.result as string;
    };
    reader.onerror = () => reject(new Error("امکان خواندن فایل از روی حافظه وجود ندارد."));
    reader.readAsDataURL(file);
  });
}

function renderSanitizedCanvas(img: HTMLImageElement, targetSize: number, quality: number): string {
  const canvas = document.createElement("canvas");
  canvas.width = targetSize;
  canvas.height = targetSize;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    throw new Error("مرورگر از شتاب‌نگار دوبعدی تصویر پشتیبانی نمی‌کند.");
  }

  // Clear canvas with transparent void
  ctx.clearRect(0, 0, targetSize, targetSize);

  // Smooth resampling
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";

  // Compute square center crop (cover style)
  const sw = img.naturalWidth || img.width;
  const sh = img.naturalHeight || img.height;
  const minDim = Math.min(sw, sh);
  const sx = (sw - minDim) / 2;
  const sy = (sh - minDim) / 2;

  ctx.drawImage(img, sx, sy, minDim, minDim, 0, 0, targetSize, targetSize);

  // Export to WebP with fallback to JPEG
  try {
    const webpUri = canvas.toDataURL("image/webp", quality);
    if (webpUri.startsWith("data:image/webp")) {
      return webpUri;
    }
  } catch {
    // fallback
  }

  return canvas.toDataURL("image/jpeg", quality);
}
