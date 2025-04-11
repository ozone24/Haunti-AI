import { BN } from 'bn.js';
import { 
  groth16, 
  zKey, 
  type Groth16Proof, 
  type ZKey, 
  type WitnessCalculator 
} from 'snarkjs';
import { 
  CircuitConfig, 
  ProofInputs, 
  ZKProverConfig 
} from './types';
import { 
  fetchWithCache, 
  generateInputHash,
  decompressWasm,
  bufferToHex
} from './utils';
import {
  ZkProofError,
  CircuitError,
  VerificationError,
  NetworkError
} from './errors';

// Core ZK Proof Manager
export class ZKProofManager {
  private readonly circuitConfig: Record<string, CircuitConfig>;
  private zkeyCache: Map<string, ZKey>;
  private wasmCache: Map<string, ArrayBuffer>;
  private vKeyCache: Map<string, any>;

  constructor(config: ZKProverConfig) {
    this.circuitConfig = config.circuits;
    this.zkeyCache = new Map();
    this.wasmCache = new Map();
    this.vKeyCache = new Map();
  }

  // Initialize circuit parameters
  public async initializeCircuit(
    circuitName: string,
    options?: {
      forceReload?: boolean;
      progressCallback?: (percent: number) => void;
    }
  ): Promise<void> {
    const config = this.circuitConfig[circuitName];
    if (!config) throw new CircuitError(`Circuit ${circuitName} not configured`);

    try {
      // Load WASM
      if (!this.wasmCache.has(circuitName) || options?.forceReload) {
        const wasmResponse = await fetchWithCache(
          config.wasmUrl, 
          'no-cache',
          options?.progressCallback
        );
        const wasmBuffer = await decompressWasm(await wasmResponse.arrayBuffer());
        this.wasmCache.set(circuitName, wasmBuffer);
      }

      // Load ZKey
      if (!this.zkeyCache.has(circuitName) || options?.forceReload) {
        const zkeyResponse = await fetchWithCache(
          config.zkeyUrl,
          'no-cache',
          options?.progressCallback
        );
        this.zkeyCache.set(circuitName, await zkeyResponse.arrayBuffer());
      }

      // Load Verification Key
      if (!this.vKeyCache.has(circuitName) || options?.forceReload) {
        const vkeyResponse = await fetch(config.vkeyUrl);
        this.vKeyCache.set(circuitName, await vkeyResponse.json());
      }
    } catch (error) {
      throw new NetworkError(`Circuit init failed: ${error.message}`);
    }
  }

  // Generate ZK Proof
  public async generateProof(
    circuitName: string,
    inputs: ProofInputs,
    options?: {
      compressProof?: boolean;
      timeout?: number;
    }
  ): Promise<{
    proof: Groth16Proof;
    publicSignals: string[];
    circuitHash: string;
  }> {
    if (!this.zkeyCache.has(circuitName)) {
      await this.initializeCircuit(circuitName);
    }

    try {
      const wasm = this.wasmCache.get(circuitName)!;
      const zkey = this.zkeyCache.get(circuitName)!;
      
      // Generate witness
      const witnessCalculator = await (
        await WebAssembly.instantiate(wasm)
      ).instance.exports as unknown as WitnessCalculator;
      
      const witnessBuffer = await witnessCalculator.calculateWTNSBin(
        inputs,
        0 // Logging level
      );

      // Generate proof
      const { proof, publicSignals } = await groth16.prove(
        new Uint8Array(zkey),
        witnessBuffer,
        null, // Use internal FFT
        options?.timeout
      );

      // Generate circuit hash
      const circuitHash = generateInputHash({
        inputs,
        wasmHash: bufferToHex(await crypto.subtle.digest('SHA-256', wasm)),
        zkeyHash: bufferToHex(await crypto.subtle.digest('SHA-256', zkey))
      });

      return {
        proof: options?.compressProof ? this.compressProof(proof) : proof,
        publicSignals,
        circuitHash
      };
    } catch (error) {
      throw new ZkProofError(`Proof generation failed: ${error.message}`);
    }
  }

  // Verify proof on-chain compatible format
  public async verifyProof(
    circuitName: string,
    proof: Groth16Proof,
    publicSignals: string[]
  ): Promise<boolean> {
    const vkey = this.vKeyCache.get(circuitName);
    if (!vkey) throw new CircuitError(`Verification key not loaded for ${circuitName}`);

    try {
      return await groth16.verify(
        vkey,
        publicSignals,
        this.decompressProof(proof)
      );
    } catch (error) {
      throw new VerificationError(`Proof verification failed: ${error.message}`);
    }
  }

  // Prepare Solana instruction for proof submission
  public createProofInstruction(
    publicSignals: string[],
    proof: Groth16Proof,
    circuitHash: string
  ): any /* Replace with actual Solana instruction type */ {
    return {
      data: this.serializeProof(publicSignals, proof),
      metadata: {
        circuitHash,
        timestamp: new Date().toISOString(),
        proverVersion: 'haunti-zkp-1.0'
      }
    };
  }

  // Proof serialization for on-chain storage
  private serializeProof(
    publicSignals: string[],
    proof: Groth16Proof
  ): Uint8Array {
    const encoder = new TextEncoder();
    return encoder.encode(JSON.stringify({
      a: [proof.pi_a[0], proof.pi_a[1]],
      b: [[proof.pi_b[0][0], proof.pi_b[0][1]], [proof.pi_b[1][0], proof.pi_b[1][1]]],
      c: [proof.pi_c[0], proof.pi_c[1]],
      signals: publicSignals
    }));
  }

  // Compress proof for efficient storage
  private compressProof(proof: Groth16Proof): Groth16Proof {
    return {
      pi_a: [new BN(proof.pi_a[0]).toString(), new BN(proof.pi_a[1]).toString()],
      pi_b: [
        [new BN(proof.pi_b[0][0]).toString(), new BN(proof.pi_b[0][1]).toString()],
        [new BN(proof.pi_b[1][0]).toString(), new BN(proof.pi_b[1][1]).toString()]
      ],
      pi_c: [new BN(proof.pi_c[0]).toString(), new BN(proof.pi_c[1]).toString()],
      protocol: proof.protocol
    };
  }

  // Decompress proof for verification
  private decompressProof(proof: Groth16Proof): Groth16Proof {
    return {
      pi_a: [new BN(proof.pi_a[0]), new BN(proof.pi_a[1])],
      pi_b: [
        [new BN(proof.pi_b[0][0]), new BN(proof.pi_b[0][1])],
        [new BN(proof.pi_b[1][0]), new BN(proof.pi_b[1][1])]
      ],
      pi_c: [new BN(proof.pi_c[0]), new BN(proof.pi_c[1])],
      protocol: proof.protocol
    };
  }
}

// Supported Circuits
export const HAUNTI_CIRCUITS: Record<string, CircuitConfig> = {
  training: {
    wasmUrl: 'https://cdn.haunti.xyz/circuits/training_js/training.wasm',
    zkeyUrl: 'https://cdn.haunti.xyz/circuits/training.zkey',
    vkeyUrl: 'https://cdn.haunti.xyz/circuits/training.vkey.json',
    inputSchema: {
      modelHash: 'string',
      dataRoot: 'string',
      hyperparams: 'object'
    }
  },
  inference: {
    wasmUrl: 'https://cdn.haunti.xyz/circuits/inference_js/inference.wasm',
    zkeyUrl: 'https://cdn.haunti.xyz/circuits/inference.zkey',
    vkeyUrl: 'https://cdn.haunti.xyz/circuits/inference.vkey.json',
    inputSchema: {
      modelHash: 'string',
      inputHash: 'string',
      outputHash: 'string'
    }
  }
};

// Utility Functions
export function validateProofInputs(
  circuitName: string,
  inputs: Record<string, any>
): boolean {
  const schema = HAUNTI_CIRCUITS[circuitName]?.inputSchema;
  if (!schema) return false;

  for (const [key, type] of Object.entries(schema)) {
    if (typeof inputs[key] !== type) return false;
  }
  return true;
}

export async function generateCircuitIdentifier(
  circuitName: string,
  version: string = 'latest'
): Promise<string> {
  const config = HAUNTI_CIRCUITS[circuitName];
  if (!config) throw new CircuitError(`Unknown circuit: ${circuitName}`);

  const wasmHash = bufferToHex(
    await crypto.subtle.digest(
      'SHA-256',
      await (await fetch(config.wasmUrl)).arrayBuffer()
    )
  );
  
  const zkeyHash = bufferToHex(
    await crypto.subtle.digest(
      'SHA-256',
      await (await fetch(config.zkeyUrl)).arrayBuffer()
    )
  );

  return `${circuitName}-${version}-${wasmHash.slice(0, 8)}-${zkeyHash.slice(0, 8)}`;
}

// Type Definitions
export type HauntiProof = {
  serializedProof: Uint8Array;
  publicSignals: string[];
  circuitIdentifier: string;
  timestamp: number;
};

export type ProofVerificationResult = {
  isValid: boolean;
  verificationTime: number;
  error?: string;
};
