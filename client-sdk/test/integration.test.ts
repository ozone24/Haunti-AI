import { AnchorProvider, BN, Program, web3, Wallet } from '@coral-xyz/anchor';
import { HauntiClient, HauntiError, TaskStatus } from './client';
import { HauntiCrypto } from './utils/crypto';
import { IPFSClient } from './utils/ipfs';
import { 
  CreateTaskArgs,
  ModelMetadata,
  ProofSubmission,
  StakingPoolType 
} from './models';
import { 
  PublicKey, 
  Keypair, 
  Connection, 
  LAMPORTS_PER_SOL,
  SystemProgram,
  Transaction
} from '@solana/web3.js';
import { expect, use } from 'chai';
import chaiAsPromised from 'chai-as-promised';
import { create } from 'ipfs-http-client';
import * as anchor from '@coral-xyz/anchor';
import { existsSync, unlinkSync } from 'fs';

use(chaiAsPromised);

describe('Haunti Integration Tests', () => {
  let provider: AnchorProvider;
  let client: HauntiClient;
  let program: Program;
  let payer: Keypair;
  let ipfs: IPFSClient;
  let crypto: HauntiCrypto;
  
  const programId = new PublicKey('HUNT1AFK4wGWqHmNYrvdcHWB8EwqyZFTzY7VWj6p3R1');
  const connection = new Connection('http://localhost:8899', 'confirmed');
  const localnetUrl = 'http://localhost:8899';

  before(async () => {
    // Initialize local validator
    if (!process.env.CI) {
      await startLocalValidator();
    }

    payer = Keypair.generate();
    provider = new AnchorProvider(
      connection,
      new Wallet(payer),
      { commitment: 'confirmed' }
    );
    
    // Fund payer account
    await requestAirdrop(payer.publicKey, 100 * LAMPORTS_PER_SOL);

    // Initialize real dependencies
    ipfs = new IPFSClient({ host: 'localhost', port: 5001, protocol: 'http' });
    crypto = new HauntiCrypto();
    program = new Program(IDL, programId, provider);
    client = new HauntiClient({
      connection,
      wallet: payer,
      crypto,
      ipfs,
      programId,
    });
  });

  after(async () => {
    // Cleanup resources
    if (existsSync('.anchor')) {
      unlinkSync('.anchor/test-ledger');
    }
  });

  async function requestAirdrop(pubkey: PublicKey, lamports: number) {
    const sig = await connection.requestAirdrop(pubkey, lamports);
    await connection.confirmTransaction(sig);
  }

  async function startLocalValidator() {
    const { execSync } = require('child_process');
    execSync('solana-test-validator --reset --quiet &', { stdio: 'inherit' });
    await new Promise(resolve => setTimeout(resolve, 5000)); // Wait for validator init
  }

  describe('Full Workflow: Training Task Lifecycle', () => {
    let taskId: PublicKey;
    const modelHash = crypto.sha256(new TextEncoder().encode('model_v1'));
    const taskArgs = new CreateTaskArgs({
      modelHash,
      maxDuration: 3600, // 1 hour
      reward: new BN(100_000_000), // 100 HAUNT tokens
    });

    it('should create a training task on-chain', async () => {
      const tx = await client.createTask(taskArgs);
      const [taskAddr] = await PublicKey.findProgramAddress(
        [Buffer.from('task'), payer.publicKey.toBuffer(), modelHash],
        programId
      );
      taskId = taskAddr;

      const taskAccount = await program.account.trainingTask.fetch(taskId);
      expect(taskAccount.status).to.equal('Pending');
      expect(taskAccount.modelHash).to.eql(modelHash);
    });

    it('should allow worker to claim and process task', async () => {
      // Worker claims task
      const worker = Keypair.generate();
      await requestAirdrop(worker.publicKey, LAMPORTS_PER_SOL);
      
      const claimTx = await program.methods
        .claimTask()
        .accounts({ task: taskId, worker: worker.publicKey })
        .signers([worker])
        .rpc();
      
      const taskAfterClaim = await program.account.trainingTask.fetch(taskId);
      expect(taskAfterClaim.status).to.equal('InProgress');
      expect(taskAfterClaim.worker?.equals(worker.publicKey)).to.be.true;
    });

    it('should submit valid ZK proof with encrypted result', async () => {
      const worker = Keypair.generate();
      const result = new TextEncoder().encode('encrypted_model_weights');
      const encryptedResult = await crypto.encryptSymmetric(
        taskArgs.modelHash, 
        result
      );
      
      const proof = new Uint8Array(512); // Mock ZK proof
      const submission = new ProofSubmission({
        taskId,
        zkProof: proof,
        encryptedResult,
      });

      await client.submitProof(submission);

      const taskAfterSubmit = await program.account.trainingTask.fetch(taskId);
      expect(taskAfterSubmit.status).to.equal('Completed');
      expect(taskAfterSubmit.result).to.eql(encryptedResult);
    });

    it('should mint ModelNFT after successful completion', async () => {
      const metadata: ModelMetadata = {
        name: 'TestModel',
        cid: 'QmTestCID123',
        version: 1,
        timestamp: Math.floor(Date.now() / 1000),
      };
      
      const mintTx = await client.mintModelNFT(taskId, metadata);
      const nftAccount = await program.account.modelNft.fetch(mintTx.mint);
      
      expect(nftAccount.modelHash).to.eql(modelHash);
      expect(nftAccount.metadataUri).to.include(metadata.cid);
    });
  });

  describe('Cross-Module Interactions', () => {
    it('should stake tokens and earn rewards from GPU pool', async () => {
      const stakeAmount = new BN(1_000_000_000); // 1000 HAUNT
      const poolType = StakingPoolType.GPUProvider;
      
      const initialBalance = await client.getTokenBalance(payer.publicKey);
      await client.stakeTokens(poolType, stakeAmount);
      
      // Simulate time passage (1 epoch)
      await program.methods
        .simulateEpoch()
        .rpc();

      const rewards = await client.calculateRewards(payer.publicKey, poolType);
      expect(rewards.gt(new BN(0))).to.be.true;

      await client.unstakeTokens(poolType, stakeAmount);
      const finalBalance = await client.getTokenBalance(payer.publicKey);
      expect(finalBalance.sub(initialBalance).gte(rewards)).to.be.true;
    });

    it('should enforce model versioning through NFT updates', async () => {
      const modelV1 = await client.createModelVersion('Initial Model');
      const modelV2 = await client.updateModelVersion(modelV1.mint, 'Improved Model');
      
      const nftV2 = await program.account.modelNft.fetch(modelV2.mint);
      expect(nftV2.version).to.equal(2);
      expect(nftV2.previousVersion?.equals(modelV1.mint)).to.be.true;
    });
  });

  describe('Failure Scenarios', () => {
    it('should slash stake for invalid proofs', async () => {
      const worker = Keypair.generate();
      await requestAirdrop(worker.publicKey, LAMPORTS_PER_SOL);
      
      // Stake first
      await client.stakeTokens(StakingPoolType.Validator, new BN(1_000_000));
      
      // Submit invalid proof
      const fakeProof = new ProofSubmission({
        taskId: PublicKey.default,
        zkProof: new Uint8Array(0),
        encryptedResult: new Uint8Array(0),
      });
      
      await expect(client.submitProof(fakeProof))
        .to.be.rejectedWith(/Proof verification failed/);

      const slashEvent = program.addEventListener('StakeSlashed', (event) => {
        expect(event.amount.toNumber()).to.be.greaterThan(0);
      });
      
      // Verify stake reduction
      const postStake = await client.getStakedAmount(worker.publicKey);
      expect(postStake.lt(new BN(1_000_000))).to.be.true;
      program.removeEventListener(slashEvent);
    });

    it('should handle IPFS failures during metadata upload', async () => {
      const badIPFS = new IPFSClient({ host: 'invalid-host', port: 9999 });
      const badClient = new HauntiClient({
        connection,
        wallet: payer,
        crypto,
        ipfs: badIPFS,
        programId,
      });

      await expect(badClient.uploadModelMetadata({} as ModelMetadata))
        .to.be.rejectedWith(/IPFS connection failed/);
    });
  });

  describe('Performance Benchmarks', function() {
    this.timeout(60_000); // Extend timeout

    it('should process 50 concurrent task submissions', async () => {
      const tasks = Array(50).fill(null).map((_, i) => 
        new CreateTaskArgs({
          modelHash: crypto.sha256(new TextEncoder().encode(`model_${i}`)),
          maxDuration: 60,
          reward: new BN(10_000),
        })
      );

      const results = await Promise.allSettled(
        tasks.map(task => client.createTask(task))
      );

      const successes = results.filter(r => r.status === 'fulfilled').length;
      expect(successes).to.equal(50);
    });

    it('should handle 1000 TPS in proof verification', async () => {
      // Requires GPU acceleration enabled
      const proofs = Array(1000).fill(null).map(() => 
        new ProofSubmission({
          taskId: PublicKey.default,
          zkProof: crypto.generateDummyProof(),
          encryptedResult: new Uint8Array(1024),
        })
      );

      const start = Date.now();
      await Promise.all(proofs.map(p => client.submitProof(p)));
      const duration = Date.now() - start;

      expect(duration).to.be.lessThan(10_000); // <10s for 1000 proofs
    });
  });
});
