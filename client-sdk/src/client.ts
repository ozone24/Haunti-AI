import { AnchorProvider, Program, web3 } from '@coral-xyz/anchor';
import { Connection, Keypair, PublicKey, SystemProgram } from '@solana/web3.js';
import { HauntiCore, IDL } from './haunti_core';
import { BN } from 'bn.js';

// Type Definitions
export type CreateTaskParams = {
  modelHash: string;
  datasetUri: string;
  maxComputeUnits: number;
  rewardAmount: BN;
  deadline: number;
};

export type SubmitProofParams = {
  taskId: web3.PublicKey;
  zkProof: Buffer;
  encryptedResult: string;
  computeUnits: number;
};

export type StakeParams = {
  poolType: 'gpu' | 'validator' | 'trainer';
  amount: BN;
  lockupPeriod: number;
};

export type ModelNFTMetadata = {
  name: string;
  symbol: string;
  modelUri: string;
  royaltyBps: number;
  encryptedParams: string;
};

// Configuration
const HAUNTI_PROGRAM_ID = new PublicKey('HAUNT1...');
const METAPLEX_PROGRAM_ID = new PublicKey('meta...');
const RPC_ENDPOINT = process.env.RPC_ENDPOINT || 'https://api.mainnet.solana.com';

export class HauntiClient {
  private connection: Connection;
  private provider: AnchorProvider;
  private program: Program<HauntiCore>;
  private wallet: web3.Signer;

  constructor(wallet: web3.Signer, opts?: web3.ConfirmOptions) {
    this.connection = new Connection(RPC_ENDPOINT, 'confirmed');
    this.provider = new AnchorProvider(this.connection, wallet, opts || {});
    this.program = new Program<HauntiCore>(IDL, HAUNTI_PROGRAM_ID, this.provider);
    this.wallet = wallet;
  }

  // Core Methods
  async createTask(params: CreateTaskParams): Promise<web3.TransactionSignature> {
    const [taskPda] = await this.findTaskAddress(params.modelHash);
    const [vaultPda] = await this.findTaskVaultAddress(taskPda);

    return await this.program.methods
      .createTask({
        ...params,
        deadline: new BN(params.deadline),
      })
      .accounts({
        task: taskPda,
        vault: vaultPda,
        payer: this.wallet.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([this.wallet])
      .rpc({ skipPreflight: true });
  }

  async submitProof(params: SubmitProofParams): Promise<web3.TransactionSignature> {
    const [verifierPda] = await this.findVerifierAddress(params.taskId);
    const [rewardPda] = await this.findRewardAddress(params.taskId);

    return await this.program.methods
      .submitProof({
        zkProof: Array.from(params.zkProof),
        encryptedResult: params.encryptedResult,
        computeUnits: params.computeUnits,
      })
      .accounts({
        task: params.taskId,
        verifier: verifierPda,
        rewardPool: rewardPda,
        submitter: this.wallet.publicKey,
      })
      .signers([this.wallet])
      .rpc();
  }

  async stakeTokens(params: StakeParams): Promise<web3.TransactionSignature> {
    const [poolPda] = await this.findPoolAddress(params.poolType);
    const [userStakePda] = await this.findUserStakeAddress(poolPda);

    return await this.program.methods
      .stake({
        amount: params.amount,
        lockupPeriod: new BN(params.lockupPeriod),
      })
      .accounts({
        pool: poolPda,
        userStake: userStakePda,
        staker: this.wallet.publicKey,
      })
      .signers([this.wallet])
      .rpc();
  }

  async mintModelNFT(metadata: ModelNFTMetadata): Promise<web3.TransactionSignature> {
    const [modelPda] = await this.findModelAddress(metadata.modelUri);
    const [metadataPda] = await this.findMetadataAddress(modelPda);

    return await this.program.methods
      .mintModelNft({
        name: metadata.name,
        symbol: metadata.symbol,
        uri: metadata.modelUri,
        royaltyBps: metadata.royaltyBps,
        encryptedParams: metadata.encryptedParams,
      })
      .accounts({
        model: modelPda,
        metadata: metadataPda,
        mintAuthority: this.wallet.publicKey,
        metaplexProgram: METAPLEX_PROGRAM_ID,
      })
      .signers([this.wallet])
      .rpc();
  }

  // PDA Derivation
  private async findTaskAddress(modelHash: string): Promise<[web3.PublicKey, number]> {
    return web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from('task'),
        Buffer.from(modelHash),
        this.wallet.publicKey.toBuffer(),
      ],
      HAUNTI_PROGRAM_ID
    );
  }

  private async findPoolAddress(poolType: string): Promise<[web3.PublicKey, number]> {
    return web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from('pool'),
        Buffer.from(poolType),
      ],
      HAUNTI_PROGRAM_ID
    );
  }

  // Event Listeners
  watchTaskUpdates(callback: (event: any) => void): number {
    return this.program.addEventListener('TaskCreated', (event) => {
      callback(this.parseTaskEvent(event));
    });
  }

  watchStakingEvents(callback: (event: any) => void): number {
    return this.program.addEventListener('Staked', (event) => {
      callback(this.parseStakeEvent(event));
    });
  }

  // Utilities
  private parseTaskEvent(event: any): TaskEvent {
    return {
      taskId: event.taskId,
      modelHash: event.modelHash,
      reward: new BN(event.reward),
      deadline: new Date(event.deadline * 1000),
    };
  }

  static async createWithKeypair(keypair: Keypair): Promise<HauntiClient> {
    const wallet = {
      publicKey: keypair.publicKey,
      signer: keypair,
    } as web3.Signer;
    return new HauntiClient(wallet);
  }
}

// Error Handling
export class HauntiError extends Error {
  constructor(
    public readonly code: number,
    public readonly msg: string,
    public readonly txSig?: string
  ) {
    super(`${code}: ${msg} (TX: ${txSig})`);
  }
}

// Usage Example
const keypair = Keypair.fromSecretKey(/* loaded from env */);
const client = await HauntiClient.createWithKeypair(keypair);

const taskSig = await client.createTask({
  modelHash: 'a1b2c3...',
  datasetUri: 'ipfs://...',
  maxComputeUnits: 1000,
  rewardAmount: new BN(100_000_000), // 100 tokens
  deadline: Math.floor(Date.now() / 1000) + 86400, // 24h
});

const listenerId = client.watchTaskUpdates((event) => {
  console.log('New task created:', event.taskId);
});
