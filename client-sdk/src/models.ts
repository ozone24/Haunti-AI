import { PublicKey } from '@solana/web3.js';
import { BN } from 'bn.js';
import { z } from 'zod';

/**
 * Core Data Models for Haunti AI Protocol
 */

// ====================
// Task Models
// ====================

export type TaskType = 'TRAINING' | 'INFERENCE';

export interface TaskParams {
  modelHash: string;
  datasetUri: string;
  maxComputeUnits: number;
  rewardAmount: BN;
  deadline: number;
  taskType: TaskType;
}

export const TaskParamsSchema = z.object({
  modelHash: z.string().length(64),
  datasetUri: z.string().url(),
  maxComputeUnits: z.number().positive(),
  rewardAmount: z.instanceof(BN),
  deadline: z.number().int().positive(),
  taskType: z.enum(['TRAINING', 'INFERENCE']),
});

export class HauntiTask {
  constructor(
    public readonly address: PublicKey,
    public readonly params: TaskParams,
    public readonly status: 'PENDING' | 'ACTIVE' | 'COMPLETED' | 'FAILED',
    public readonly createdAt: Date
  ) {}

  static fromAccountInfo(accountInfo: any): HauntiTask {
    return new HauntiTask(
      accountInfo.publicKey as PublicKey,
      {
        modelHash: accountInfo.modelHash.toString('utf8'),
        datasetUri: accountInfo.datasetUri,
        maxComputeUnits: accountInfo.maxComputeUnits,
        rewardAmount: new BN(accountInfo.rewardAmount),
        deadline: accountInfo.deadline.toNumber(),
        taskType: accountInfo.taskType === 0 ? 'TRAINING' : 'INFERENCE',
      },
      accountInfo.status,
      new Date(accountInfo.createdAt.toNumber() * 1000)
    );
  }
}

// ====================
// Proof & Computation
// ====================

export interface ZKProof {
  proof: Buffer;
  publicInputs: string[];
  circuitId: string;
}

export interface EncryptedResult {
  ciphertext: string;
  dataHash: string;
  encryptionParams: {
    scheme: 'FHE-BGV' | 'FHE-CKKS';
    polyModulusDegree?: number;
    coeffModulus?: string;
  };
}

// ====================
// Staking Models
// ====================

export type PoolType = 'GPU' | 'VALIDATOR' | 'TRAINER';

export interface StakePosition {
  pool: PublicKey;
  amount: BN;
  lockStart: Date;
  lockEnd: Date;
  rewardsClaimed: BN;
}

export const StakeParamsSchema = z.object({
  poolType: z.enum(['GPU', 'VALIDATOR', 'TRAINER']),
  amount: z.instanceof(BN).refine(b => b.gt(new BN(0))),
  lockupPeriod: z.number().int().min(0),
});

// ====================
// NFT Models
// ====================

export interface ModelNFTMetadata {
  name: string;
  symbol: string;
  modelUri: string;
  royaltyBps: number;
  encryptedParams: string;
  accuracy?: number;
  trainingDataHash: string;
}

export const ModelNFTMetadataSchema = z.object({
  name: z.string().min(3).max(50),
  symbol: z.string().min(3).max(6),
  modelUri: z.string().url(),
  royaltyBps: z.number().min(0).max(10000),
  encryptedParams: z.string(),
  accuracy: z.number().min(0).max(1).optional(),
  trainingDataHash: z.string().length(64),
});

// ====================
// Event Models
// ====================

export type TaskEvent = {
  taskId: PublicKey;
  modelHash: string;
  reward: BN;
  deadline: Date;
};

export type StakeEvent = {
  poolType: PoolType;
  staker: PublicKey;
  amount: BN;
  duration: number;
};

export type ProofSubmittedEvent = {
  taskId: PublicKey;
  submitter: PublicKey;
  computeUnits: number;
  reward: BN;
};

// ====================
// Error Models
// ====================

export const HAUNTI_ERROR_CODES = {
  INVALID_TASK_STATE: 1001,
  PROOF_VERIFICATION_FAILED: 2001,
  INSUFFICIENT_REWARDS: 3001,
  NFT_METADATA_INVALID: 4001,
} as const;

export type HauntiErrorCode = typeof HAUNTI_ERROR_CODES[keyof typeof HAUNTI_ERROR_CODES];

export class HauntiError extends Error {
  constructor(
    public code: HauntiErrorCode,
    public message: string,
    public context?: Record<string, any>
  ) {
    super(`[${code}] ${message}`);
    Object.setPrototypeOf(this, HauntiError.prototype);
  }

  static fromAnchorError(error: any): HauntiError {
    return new HauntiError(
      error.code,
      error.msg,
      error.accounts ? { accounts: error.accounts } : undefined
    );
  }
}

// ====================
// Utility Types
// ====================

export type PaginatedResult<T> = {
  items: T[];
  nextCursor?: PublicKey;
};

export type RewardBreakdown = {
  pending: BN;
  claimed: BN;
  lifetime: BN;
};

// ====================
// API Response Models
// ====================

export interface TaskListResponse {
  tasks: HauntiTask[];
  nextPage?: string;
}

export interface StakePositionResponse {
  positions: StakePosition[];
  totalStaked: BN;
  totalRewards: RewardBreakdown;
}

// ====================
// Serialization Helpers
// ====================

export class BnSerializer {
  static serialize(bn: BN): string {
    return bn.toString(16);
  }

  static deserialize(hex: string): BN {
    return new BN(hex, 16);
  }
}

export class DateSerializer {
  static serialize(date: Date): number {
    return Math.floor(date.getTime() / 1000);
  }

  static deserialize(timestamp: number): Date {
    return new Date(timestamp * 1000);
  }
}
