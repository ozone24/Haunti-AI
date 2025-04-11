import { randomBytes, createHash } from 'crypto';
import { Box, sign, verify, open, seal } from 'tweetnacl-ts';
import { groth16 } from 'snarkjs';
import { Point, utils } from '@noble/curves/abstract/curve';
import { ed25519 } from '@noble/curves/ed25519';
import { secp256k1 } from '@noble/curves/secp256k1';
import { blake2b } from '@noble/hashes/blake2b';
import { hmac } from '@noble/hashes/hmac';
import { sha256 } from '@noble/hashes/sha256';
import { sha3 } from '@noble/hashes/sha3';
import { x25519 } from '@noble/curves/x25519';
import { pbkdf2Async } from '@noble/hashes/pbkdf2';
import { bytesToHex, hexToBytes, utf8ToBytes } from 'ethereum-cryptography/utils';
import { z } from 'zod';

// ====================
// Type Definitions
// ====================

type KeyPair = {
  publicKey: Uint8Array;
  secretKey: Uint8Array;
};

type ECIESEncrypted = {
  ciphertext: Uint8Array;
  ephemPublicKey: Uint8Array;
  nonce: Uint8Array;
  mac: Uint8Array;
};

type ZKProof = {
  proof: any;
  publicSignals: any;
};

// ====================
// Configuration Schema
// ====================

const CryptoConfigSchema = z.object({
  encryptionAlgorithm: z.enum(['x25519-xsalsa20-poly1305', 'secp256k1-ecies']).default('x25519-xsalsa20-poly1305'),
  hashFunction: z.enum(['blake2b', 'sha256', 'sha3-256']).default('blake2b'),
  symmetricAlgorithm: z.enum(['aes-256-gcm', 'chacha20-poly1305']).default('aes-256-gcm'),
  saltLength: z.number().min(16).max(64).default(32),
  pbkdf2Iterations: z.number().min(10000).max(1000000).default(100000),
});

type CryptoConfig = z.infer<typeof CryptoConfigSchema>;

// ====================
// Custom Errors
// ====================

export class CryptoError extends Error {
  constructor(
    public code: string,
    message: string,
    public metadata?: Record<string, any>
  ) {
    super(`[Crypto-${code}] ${message}`);
    Object.setPrototypeOf(this, CryptoError.prototype);
  }
}

// ====================
// Core Crypto Class
// ====================

export class HauntiCrypto {
  private config: CryptoConfig;

  constructor(config?: Partial<CryptoConfig>) {
    this.config = CryptoConfigSchema.parse(config || {});
  }

  // ====================
  // Key Generation
  // ====================

  generateKeyPair(curve: 'ed25519' | 'secp256k1' | 'x25519'): KeyPair {
    try {
      switch (curve) {
        case 'ed25519':
          return ed25519.utils.randomPrivateKey().then(priv => ({
            secretKey: priv,
            publicKey: ed25519.getPublicKey(priv),
          }));
        case 'secp256k1':
          const priv = secp256k1.utils.randomPrivateKey();
          return {
            secretKey: priv,
            publicKey: secp256k1.getPublicKey(priv, true),
          };
        case 'x25519':
          const seed = randomBytes(32);
          return {
            secretKey: seed,
            publicKey: x25519.getPublicKey(seed),
          };
        default:
          throw new CryptoError('UNSUPPORTED_CURVE', `Unsupported curve: ${curve}`);
      }
    } catch (err) {
      throw new CryptoError('KEYGEN_FAILED', 'Key generation failed', {
        curve,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  // ====================
  // Asymmetric Encryption
  // ====================

  async encryptAsymmetric(
    publicKey: Uint8Array,
    data: Uint8Array,
    options?: { 
      algorithm?: 'x25519-xsalsa20-poly1305' | 'secp256k1-ecies',
      associatedData?: Uint8Array 
    }
  ): Promise<Uint8Array> {
    const algorithm = options?.algorithm || this.config.encryptionAlgorithm;
    
    try {
      switch (algorithm) {
        case 'x25519-xsalsa20-poly1305':
          const ephemeral = this.generateKeyPair('x25519');
          const sharedKey = x25519.getSharedSecret(ephemeral.secretKey, publicKey);
          const nonce = randomBytes(24);
          const box = seal(data, nonce, sharedKey);
          return new Uint8Array([...ephemeral.publicKey, ...nonce, ...box]);
          
        case 'secp256k1-ecies':
          const ephemeralPriv = secp256k1.utils.randomPrivateKey();
          const ephemeralPub = secp256k1.getPublicKey(ephemeralPriv, true);
          const sharedPoint = secp256k1.getSharedSecret(ephemeralPriv, publicKey);
          const derivedKey = this.deriveSymmetricKey(sharedPoint);
          const encrypted = await this.encryptSymmetric(derivedKey, data, options);
          return new Uint8Array([...ephemeralPub, ...encrypted]);
          
        default:
          throw new CryptoError('UNSUPPORTED_ALGORITHM', `Unsupported algorithm: ${algorithm}`);
      }
    } catch (err) {
      throw new CryptoError('ASYMM_ENCRYPT_FAILED', 'Asymmetric encryption failed', {
        algorithm,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  async decryptAsymmetric(
    secretKey: Uint8Array,
    encryptedData: Uint8Array,
    options?: { 
      algorithm?: 'x25519-xsalsa20-poly1305' | 'secp256k1-ecies',
      associatedData?: Uint8Array 
    }
  ): Promise<Uint8Array> {
    const algorithm = options?.algorithm || this.config.encryptionAlgorithm;
    
    try {
      switch (algorithm) {
        case 'x25519-xsalsa20-poly1305':
          const ephemPublicKey = encryptedData.slice(0, 32);
          const nonce = encryptedData.slice(32, 56);
          const box = encryptedData.slice(56);
          const sharedKey = x25519.getSharedSecret(secretKey, ephemPublicKey);
          return open(box, nonce, sharedKey);
          
        case 'secp256k1-ecies':
          const ephemeralPub = encryptedData.slice(0, 33);
          const encryptedPayload = encryptedData.slice(33);
          const sharedPoint = secp256k1.getSharedSecret(secretKey, ephemeralPub);
          const derivedKey = this.deriveSymmetricKey(sharedPoint);
          return this.decryptSymmetric(derivedKey, encryptedPayload, options);
          
        default:
          throw new CryptoError('UNSUPPORTED_ALGORITHM', `Unsupported algorithm: ${algorithm}`);
      }
    } catch (err) {
      throw new CryptoError('ASYMM_DECRYPT_FAILED', 'Asymmetric decryption failed', {
        algorithm,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  // ====================
  // Symmetric Encryption
  // ====================

  async encryptSymmetric(
    key: Uint8Array,
    data: Uint8Array,
    options?: { 
      algorithm?: 'aes-256-gcm' | 'chacha20-poly1305',
      associatedData?: Uint8Array 
    }
  ): Promise<Uint8Array> {
    const algorithm = options?.algorithm || this.config.symmetricAlgorithm;
    const nonce = randomBytes(algorithm === 'aes-256-gcm' ? 12 : 24);
    
    try {
      switch (algorithm) {
        case 'aes-256-gcm':
          const cipher = createCipheriv('aes-256-gcm', key, nonce, { 
            authTagLength: 16 
          });
          if (options?.associatedData) {
            cipher.setAAD(options.associatedData);
          }
          const encrypted = Buffer.concat([
            cipher.update(data),
            cipher.final(),
            cipher.getAuthTag(),
          ]);
          return new Uint8Array([...nonce, ...encrypted]);
          
        case 'chacha20-poly1305':
          const cipherState = new ChaCha20Poly1305(key);
          const encrypted = cipherState.encrypt(nonce, data, options?.associatedData);
          return new Uint8Array([...nonce, ...encrypted]);
          
        default:
          throw new CryptoError('UNSUPPORTED_ALGORITHM', `Unsupported algorithm: ${algorithm}`);
      }
    } catch (err) {
      throw new CryptoError('SYMM_ENCRYPT_FAILED', 'Symmetric encryption failed', {
        algorithm,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  async decryptSymmetric(
    key: Uint8Array,
    encryptedData: Uint8Array,
    options?: { 
      algorithm?: 'aes-256-gcm' | 'chacha20-poly1305',
      associatedData?: Uint8Array 
    }
  ): Promise<Uint8Array> {
    const algorithm = options?.algorithm || this.config.symmetricAlgorithm;
    const nonce = encryptedData.slice(0, algorithm === 'aes-256-gcm' ? 12 : 24);
    const payload = encryptedData.slice(nonce.length);
    
    try {
      switch (algorithm) {
        case 'aes-256-gcm':
          const decipher = createDecipheriv('aes-256-gcm', key, nonce, {
            authTagLength: 16,
          });
          const tag = payload.slice(-16);
          const ciphertext = payload.slice(0, -16);
          if (options?.associatedData) {
            decipher.setAAD(options.associatedData);
          }
          decipher.setAuthTag(tag);
          return new Uint8Array([
            ...decipher.update(ciphertext),
            ...decipher.final(),
          ]);
          
        case 'chacha20-poly1305':
          const cipherState = new ChaCha20Poly1305(key);
          return cipherState.decrypt(nonce, payload, options?.associatedData);
          
        default:
          throw new CryptoError('UNSUPPORTED_ALGORITHM', `Unsupported algorithm: ${algorithm}`);
      }
    } catch (err) {
      throw new CryptoError('SYMM_DECRYPT_FAILED', 'Symmetric decryption failed', {
        algorithm,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  // ====================
  // Cryptographic Hashing
  // ====================

  hash(data: Uint8Array, options?: { algorithm?: 'blake2b' | 'sha256' | 'sha3-256' }): Uint8Array {
    const algorithm = options?.algorithm || this.config.hashFunction;
    
    switch (algorithm) {
      case 'blake2b':
        return blake2b(data, { dkLen: 32 });
      case 'sha256':
        return sha256(data);
      case 'sha3-256':
        return sha3(data, { dkLen: 32 });
      default:
        throw new CryptoError('UNSUPPORTED_HASH', `Unsupported hash algorithm: ${algorithm}`);
    }
  }

  // ====================
  // Digital Signatures
  // ====================

  sign(
    secretKey: Uint8Array,
    message: Uint8Array,
    curve: 'ed25519' | 'secp256k1' = 'ed25519'
  ): Uint8Array {
    try {
      switch (curve) {
        case 'ed25519':
          return ed25519.sign(message, secretKey);
        case 'secp256k1':
          return secp256k1.sign(message, secretKey).toCompactRawBytes();
        default:
          throw new CryptoError('UNSUPPORTED_CURVE', `Unsupported curve: ${curve}`);
      }
    } catch (err) {
      throw new CryptoError('SIGN_FAILED', 'Signature generation failed', {
        curve,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  verify(
    publicKey: Uint8Array,
    message: Uint8Array,
    signature: Uint8Array,
    curve: 'ed25519' | 'secp256k1' = 'ed25519'
  ): boolean {
    try {
      switch (curve) {
        case 'ed25519':
          return ed25519.verify(signature, message, publicKey);
        case 'secp256k1':
          return secp256k1.verify(signature, message, publicKey);
        default:
          throw new CryptoError('UNSUPPORTED_CURVE', `Unsupported curve: ${curve}`);
      }
    } catch (err) {
      throw new CryptoError('VERIFY_FAILED', 'Signature verification failed', {
        curve,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  // ====================
  // Key Derivation
  // ====================

  async deriveKeyFromSecret(
    secret: Uint8Array,
    salt: Uint8Array,
    iterations: number = this.config.pbkdf2Iterations
  ): Promise<Uint8Array> {
    try {
      return await pbkdf2Async(
        sha256,
        secret,
        salt,
        { c: iterations, dkLen: 32 }
      );
    } catch (err) {
      throw new CryptoError('KDF_FAILED', 'Key derivation failed', {
        iterations,
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  // ====================
  // Zero-Knowledge Proofs
  // ====================

  async generateZKProof(
    circuitWasm: Buffer,
    zkey: Buffer,
    inputs: object
  ): Promise<ZKProof> {
    try {
      const { proof, publicSignals } = await groth16.fullProve(
        inputs,
        circuitWasm,
        zkey
      );
      return { proof, publicSignals };
    } catch (err) {
      throw new CryptoError('ZK_PROOF_FAILED', 'ZK proof generation failed', {
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  async verifyZKProof(
    verificationKey: object,
    proof: any,
    publicSignals: any
  ): Promise<boolean> {
    try {
      return await groth16.verify(
        verificationKey,
        publicSignals,
        proof
      );
    } catch (err) {
      throw new CryptoError('ZK_VERIFY_FAILED', 'ZK proof verification failed', {
        error: err instanceof Error ? err.message : 'Unknown',
      });
    }
  }

  // ====================
  // Utility Methods
  // ====================

  randomBytes(length: number): Uint8Array {
    return randomBytes(length);
  }

  bytesToHex(bytes: Uint8Array): string {
    return bytesToHex(bytes);
  }

  hexToBytes(hex: string): Uint8Array {
    return hexToBytes(hex);
  }

  stringToBytes(str: string): Uint8Array {
    return utf8ToBytes(str);
  }
}

// ====================
// Usage Examples
// ====================

// Initialize with custom config
const crypto = new HauntiCrypto({
  encryptionAlgorithm: 'secp256k1-ecies',
  hashFunction: 'blake2b',
});

// Generate keys
const keyPair = crypto.generateKeyPair('secp256k1');

// Encrypt data
const encrypted = await crypto.encryptAsymmetric(
  keyPair.publicKey,
  new TextEncoder().encode('Haunti AI Secret'),
  { algorithm: 'secp256k1-ecies' }
);

// Decrypt data
const decrypted = await crypto.decryptAsymmetric(
  keyPair.secretKey,
  encrypted,
  { algorithm: 'secp256k1-ecies' }
);

// Generate ZK Proof
const circuit = await fetch('/circuit.wasm');
const zkey = await fetch('/circuit.zkey');
const { proof, publicSignals } = await crypto.generateZKProof(
  await circuit.arrayBuffer(),
  await zkey.arrayBuffer(),
  { input: 42 }
);

// Verify ZK Proof
const valid = await crypto.verifyZKProof(verificationKey, proof, publicSignals);
