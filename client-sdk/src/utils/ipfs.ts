import { create, globSource } from 'ipfs-http-client';
import { concat as uint8ArrayConcat } from 'uint8arrays/concat';
import { toString as uint8ArrayToString } from 'uint8arrays/to-string';
import { fromString as uint8ArrayFromString } from 'uint8arrays/from-string';
import { Libp2pCryptoIdentity } from 'ipfs-core-types/src/root';
import { Key } from 'interface-datastore/key';
import { MemoryDatastore } from 'datastore-core/memory';
import { AES } from 'libsodium-wrappers';
import { z } from 'zod';

// ====================
// Configuration Schema
// ====================

const IPFSConfigSchema = z.object({
  apiKey: z.string().optional(),
  endpoint: z.string().url().default('https://ipfs.haunti.ai'),
  timeout: z.number().min(1000).default(15000),
  encryptionKey: z.instanceof(Uint8Array).optional(),
});

type IPFSConfig = z.infer<typeof IPFSConfigSchema>;

// ====================
// Custom Errors
// ====================

export class IPFSError extends Error {
  constructor(
    public code: string,
    message: string,
    public metadata?: Record<string, any>
  ) {
    super(`[IPFS-${code}] ${message}`);
    Object.setPrototypeOf(this, IPFSError.prototype);
  }
}

// ====================
// Core IPFS Client
// ====================

export class IPFSClient {
  private client: ReturnType<typeof create>;
  private identity?: Libp2pCryptoIdentity;
  private datastore = new MemoryDatastore();
  private config: IPFSConfig;

  constructor(config?: Partial<IPFSConfig>) {
    this.config = IPFSConfigSchema.parse(config || {});
    this.client = create({
      url: new URL(this.config.endpoint),
      timeout: this.config.timeout.toString(),
      headers: this.config.apiKey 
        ? { Authorization: `Bearer ${this.config.apiKey}` } 
        : undefined,
    });
  }

  // ====================
  // Core Methods
  // ====================

  async uploadEncrypted(
    data: Uint8Array | string,
    options?: { 
      chunkSize?: number,
      progressCallback?: (loaded: number, total: number) => void
    }
  ): Promise<{ cid: string; decryptionKey: Uint8Array }> {
    await AES.ready;
    
    const encryptionKey = this.config.encryptionKey || AES.randomBytes(32);
    const nonce = AES.randomBytes(AES.NONCE_BYTES);
    const chunkSize = options?.chunkSize || 1024 * 1024; // 1MB chunks
    
    try {
      // Encrypt and chunk data
      const encryptedChunks: Uint8Array[] = [];
      const dataBytes = typeof data === 'string' 
        ? uint8ArrayFromString(data) 
        : data;
      
      let totalUploaded = 0;
      for (let offset = 0; offset < dataBytes.length; offset += chunkSize) {
        const chunk = dataBytes.subarray(offset, offset + chunkSize);
        const encrypted = AES.encrypt(chunk, nonce, encryptionKey);
        encryptedChunks.push(encrypted);
        
        totalUploaded += chunk.length;
        options?.progressCallback?.(totalUploaded, dataBytes.length);
      }

      // Upload chunks
      const addResults = await Promise.all(
        encryptedChunks.map(async (chunk, index) => {
          return this.client.add(chunk, {
            pin: true,
            headers: { 
              'X-Chunk-Index': index.toString(),
              'X-Nonce': uint8ArrayToString(nonce, 'base64'),
            },
          });
        })
      );

      // Store metadata
      const metadata = {
        chunks: addResults.map(r => r.cid.toString()),
        nonce: uint8ArrayToString(nonce, 'base64'),
        chunkSize,
        totalSize: dataBytes.length,
        encryptedAt: new Date().toISOString(),
      };
      
      const metadataCid = await this.client.add(uint8ArrayFromString(JSON.stringify(metadata)));
      
      return { 
        cid: metadataCid.cid.toString(),
        decryptionKey: encryptionKey 
      };
    } catch (err) {
      throw new IPFSError('UPLOAD_FAILED', 'Failed to upload encrypted data', {
        chunkSize,
        dataType: typeof data,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  async downloadDecrypted(
    cid: string,
    decryptionKey: Uint8Array,
    options?: {
      concurrency?: number,
      progressCallback?: (loaded: number, total: number) => void
    }
  ): Promise<Uint8Array> {
    await AES.ready;
    
    try {
      // Fetch metadata
      const metadataStr = uint8ArrayToString(
        uint8ArrayConcat(await this.cat(cid)),
        'utf8'
      );
      const metadata = JSON.parse(metadataStr);
      
      if (!metadata.chunks || !metadata.nonce) {
        throw new IPFSError('INVALID_METADATA', 'Missing required metadata fields');
      }

      const nonce = uint8ArrayFromString(metadata.nonce, 'base64');
      const concurrency = options?.concurrency || 5;
      const semaphore = new Semaphore(concurrency);
      
      // Download and decrypt chunks
      const decryptedChunks: Uint8Array[] = [];
      let totalDownloaded = 0;
      
      await Promise.all(metadata.chunks.map(async (chunkCid: string, index: number) => {
        await semaphore.acquire();
        
        try {
          const encryptedData = uint8ArrayConcat(await this.cat(chunkCid));
          const decrypted = AES.decrypt(encryptedData, nonce, decryptionKey);
          
          decryptedChunks[index] = decrypted;
          totalDownloaded += decrypted.length;
          options?.progressCallback?.(totalDownloaded, metadata.totalSize);
        } finally {
          semaphore.release();
        }
      }));

      // Reassemble data
      return uint8ArrayConcat(decryptedChunks);
    } catch (err) {
      throw new IPFSError('DOWNLOAD_FAILED', 'Failed to download and decrypt data', {
        cid,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  async cat(cid: string): Promise<Uint8Array[]> {
    try {
      const chunks: Uint8Array[] = [];
      for await (const chunk of this.client.cat(cid)) {
        chunks.push(chunk);
      }
      return chunks;
    } catch (err) {
      throw new IPFSError('CAT_FAILED', 'Failed to retrieve IPFS data', {
        cid,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  // ====================
  // Advanced Methods
  // ====================

  async pinDirectory(cid: string): Promise<void> {
    try {
      await this.client.pin.add(cid, { recursive: true });
    } catch (err) {
      throw new IPFSError('PIN_FAILED', 'Failed to pin directory', {
        cid,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  async storeKey(key: Uint8Array, name: string): Promise<void> {
    const keyName = new Key(`/encryption-keys/${name}`);
    await this.datastore.put(keyName, key);
  }

  async retrieveKey(name: string): Promise<Uint8Array | null> {
    const keyName = new Key(`/encryption-keys/${name}`);
    return this.datastore.get(keyName);
  }

  // ====================
  // Utility Methods
  // ====================

  async getFolderCID(folderPath: string): Promise<string> {
    try {
      const { cid } = await this.client.add(globSource(folderPath, { recursive: true }));
      return cid.toString();
    } catch (err) {
      throw new IPFSError('FOLDER_CID_FAILED', 'Failed to calculate folder CID', {
        folderPath,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  async validateCID(cid: string): Promise<boolean> {
    try {
      await this.client.files.stat(`/ipfs/${cid}`);
      return true;
    } catch {
      return false;
    }
  }
}

// ====================
// Concurrency Control
// ====================

class Semaphore {
  private queue: (() => void)[] = [];
  constructor(private concurrency: number) {}

  async acquire(): Promise<void> {
    while (this.concurrency <= 0) {
      await new Promise(resolve => this.queue.push(resolve));
    }
    this.concurrency--;
  }

  release(): void {
    this.concurrency++;
    const next = this.queue.shift();
    if (next) next();
  }
}

// ====================
// Usage Examples
// ====================

// Initialize client
const ipfs = new IPFSClient({
  apiKey: process.env.IPFS_API_KEY,
  endpoint: 'https://ipfs.haunti.ai',
});

// Upload encrypted model
const model = new Uint8Array(...);
const { cid, decryptionKey } = await ipfs.uploadEncrypted(model, {
  progressCallback: (loaded, total) => {
    console.log(`Upload: ${((loaded / total) * 100).toFixed(1)}%`);
  }
});

// Download and decrypt
const decrypted = await ipfs.downloadDecrypted(cid, decryptionKey, {
  progressCallback: (loaded, total) => {
    console.log(`Download: ${((loaded / total) * 100).toFixed(1)}%`);
  }
});
