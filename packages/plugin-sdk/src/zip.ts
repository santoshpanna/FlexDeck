/**
 * A minimal zip writer: deflate, CRC-32, and a central directory — no
 * dependencies, because Node already has both halves (`zlib`).
 *
 * Deterministic on purpose: every entry is stamped with the same time and the
 * entries are sorted, so packing the same files twice gives the same bytes and
 * therefore the same SHA-256. That is what lets the marketplace pin a checksum
 * that a reviewer can reproduce.
 */

import { crc32, deflateRawSync, inflateRawSync } from "node:zlib";

export interface ZipEntry {
  /** Forward slashes, relative, no `..`. */
  name: string;
  data: Uint8Array;
}

/** 1980-01-01 00:00, the earliest time a zip can say. */
const DOS_TIME = 0;
const DOS_DATE = (0 << 9) | (1 << 5) | 1;

export function zip(entries: readonly ZipEntry[]): Buffer {
  const sorted = [...entries].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  const locals: Buffer[] = [];
  const centrals: Buffer[] = [];
  let offset = 0;

  for (const entry of sorted) {
    if (entry.name.startsWith("/") || entry.name.split("/").some((part) => part === ".." || part === "")) {
      throw new Error(`'${entry.name}' is not a safe path inside a package`);
    }
    const name = Buffer.from(entry.name, "utf8");
    const checksum = crc32(entry.data);
    const compressed = deflateRawSync(entry.data, { level: 9 });
    // Store instead when deflate does not help (already-compressed images).
    const stored = compressed.length >= entry.data.length;
    const body = stored ? Buffer.from(entry.data) : compressed;
    const method = stored ? 0 : 8;

    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4); // version needed
    local.writeUInt16LE(0x0800, 6); // UTF-8 names
    local.writeUInt16LE(method, 8);
    local.writeUInt16LE(DOS_TIME, 10);
    local.writeUInt16LE(DOS_DATE, 12);
    local.writeUInt32LE(checksum, 14);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(entry.data.length, 22);
    local.writeUInt16LE(name.length, 26);
    local.writeUInt16LE(0, 28);
    locals.push(local, name, body);

    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4); // version made by
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt16LE(method, 10);
    central.writeUInt16LE(DOS_TIME, 12);
    central.writeUInt16LE(DOS_DATE, 14);
    central.writeUInt32LE(checksum, 16);
    central.writeUInt32LE(body.length, 20);
    central.writeUInt32LE(entry.data.length, 24);
    central.writeUInt16LE(name.length, 28);
    central.writeUInt32LE(offset, 42);
    centrals.push(central, name);

    offset += local.length + name.length + body.length;
  }

  const directory = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(sorted.length, 8);
  end.writeUInt16LE(sorted.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, directory, end]);
}

/**
 * Read one file out of a zip, by name — enough to find `plugin.json` in a
 * package. Reads the central directory, so it works for any zip, not only the
 * ones {@link zip} writes. Returns `null` when the file is not there.
 */
export function unzipEntry(archive: Uint8Array, wanted: string): Buffer | null {
  const buffer = Buffer.from(archive.buffer, archive.byteOffset, archive.byteLength);
  // The end record is the last 22 bytes, plus a comment of up to 64 KiB.
  let end = -1;
  for (let at = buffer.length - 22; at >= Math.max(0, buffer.length - 22 - 0xffff); at--) {
    if (buffer.readUInt32LE(at) === 0x06054b50) {
      end = at;
      break;
    }
  }
  if (end < 0) throw new Error("not a zip file");
  const count = buffer.readUInt16LE(end + 10);
  let at = buffer.readUInt32LE(end + 16);
  for (let index = 0; index < count; index++) {
    if (buffer.readUInt32LE(at) !== 0x02014b50) throw new Error("the zip's directory is damaged");
    const method = buffer.readUInt16LE(at + 10);
    const compressedSize = buffer.readUInt32LE(at + 20);
    const nameLength = buffer.readUInt16LE(at + 28);
    const extraLength = buffer.readUInt16LE(at + 30);
    const commentLength = buffer.readUInt16LE(at + 32);
    const localOffset = buffer.readUInt32LE(at + 42);
    const name = buffer.toString("utf8", at + 46, at + 46 + nameLength);
    if (name === wanted) {
      const localName = buffer.readUInt16LE(localOffset + 26);
      const localExtra = buffer.readUInt16LE(localOffset + 28);
      const start = localOffset + 30 + localName + localExtra;
      const body = buffer.subarray(start, start + compressedSize);
      if (method === 0) return Buffer.from(body);
      if (method === 8) return inflateRawSync(body);
      throw new Error(`'${name}' uses a compression this reader does not know (${method})`);
    }
    at += 46 + nameLength + extraLength + commentLength;
  }
  return null;
}
