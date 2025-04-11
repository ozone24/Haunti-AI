import { AnchorProvider, BN, Program, web3 } from '@coral-xyz/anchor';
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
  LAMPORTS_PER_SOL 
} from '@solana/web3.js';
import { expect, use } from 'chai';
import chaiAsPromised from 'chai-as-promised';
import { mock, instance, when, verify, reset } from 'ts-mockito';

use(chaiAsPromised);

// Mock Dependencies
const mockProvider = mock(AnchorProvider);
const mockProgram = mock(Program);
const mockCrypto = mock(HauntiCrypto);
const mockIPFS = mock(IPFSClient);

describe('Haunti SDK Unit Tests', () => {
  let client: HauntiClient;
  const payer = Keypair.generate();
  const programId = new PublicKey('HUNT1AFK4wGWqHmNYrvdcHWB8EwqyZFTzY7VWj6p3R1');
  const connection = new Connection('http://localhost:8899');

  beforeEach(() => {
    reset(mockProvider);
    reset(mockProgram);
    reset(mockCrypto);
    reset(mockIPFS);

    client = new HauntiClient({
      connection,
      wallet: payer,
      crypto: instance(mockCrypto),
      ipfs: instance(mockIPFS),
      programId,
    });
  });

  describe('Core Functionality', () => {
    it('should initialize client with valid parameters', () => {
      expect(client).to.be.instanceOf(HauntiClient);
      expect(client.program.programId.equals(programId)).to.be.true;
    });

    it('should throw when creating task with invalid parameters', async () => {
      const invalidTask = new CreateTaskArgs({
        modelHash: new Uint8Array(32), // Zero hash
        maxDuration: 0, // Invalid duration
      });

      await expect(
        client.createTask(invalidTask)
      ).to.be.rejectedWith(HauntiError, 'Invalid task parameters');
    });
  });

  describe('Task Lifecycle', () => {
    const taskArgs = new CreateTaskArgs({
      modelHash: crypto.getRandomValues(new Uint8Array(32)),
      maxDuration: 3600,
    });

    it('should successfully create a task', async () => {
      const mockTxId = 'test_tx_123';
      when(mockProgram.methods.createTask(taskArgs))
        .returns({ rpc: () => Promise.resolve(mockTxId) });

      const txId = await client.createTask(taskArgs);
      expect(txId).to.equal(mockTxId);
    });

    it('should handle task creation failure', async () => {
      when(mockProgram.methods.createTask(taskArgs))
        .throws(new Error('RPC failed'));

      await expect(client.createTask(taskArgs))
        .to.be.rejectedWith('RPC failed');
    });
  });

  describe('Proof Submission', () => {
    const proof = new ProofSubmission({
      taskId: new PublicKey(Keypair.generate().publicKey),
      zkProof: new Uint8Array(256),
      encryptedResult: new Uint8Array(1024),
    });

    it('should submit valid proof', async () => {
      const mockTxId = 'proof_tx_456';
      when(mockCrypto.verifyZKProof(any, any, any))
        .resolves(true);
      when(mockProgram.methods.submitProof(proof))
        .returns({ rpc: () => mockTxId });

      const txId = await client.submitProof(proof);
      expect(txId).to.equal(mockTxId);
    });

    it('should reject invalid ZK proof', async () => {
      when(mockCrypto.verifyZKProof(any, any, any))
        .resolves(false);

      await expect(client.submitProof(proof))
        .to.be.rejectedWith(HauntiError, 'Invalid ZK proof');
    });
  });

  describe('Staking & Rewards', () => {
    const poolType = StakingPoolType.GPUProvider;
    const amount = new BN(1000);

    it('should stake tokens successfully', async () => {
      const mockTxId = 'stake_tx_789';
      when(mockProgram.methods.stake(poolType, amount))
        .returns({ rpc: () => mockTxId });

      const txId = await client.stakeTokens(poolType, amount);
      expect(txId).to.equal(mockTxId);
    });

    it('should calculate APY correctly', async () => {
      const expectedAPY = 15.2;
      when(mockProgram.account.stakingPool.fetch(any))
        .resolves({ apy: expectedAPY });

      const apy = await client.getPoolAPY(poolType);
      expect(apy).to.equal(expectedAPY);
    });
  });

  describe('IPFS Integration', () => {
    const testData = { model: 'test_v1' };
    const testCID = 'QmTestCID123';

    it('should upload and pin model metadata', async () => {
      when(mockIPFS.uploadJSON(testData))
        .resolves(testCID);

      const cid = await client.uploadModelMetadata(testData);
      expect(cid).to.equal(testCID);
      verify(mockIPFS.pin(cid)).once();
    });

    it('should handle IPFS upload failure', async () => {
      when(mockIPFS.uploadJSON(testData))
        .rejects(new Error('IPFS timeout'));

      await expect(client.uploadModelMetadata(testData))
        .to.be.rejectedWith('IPFS timeout');
    });
  });

  describe('Cryptography', () => {
    const data = new TextEncoder().encode('haunti_secret');
    const keyPair = {
      publicKey: new Uint8Array(32),
      secretKey: new Uint8Array(64),
    };

    it('should encrypt/decrypt symmetrically', async () => {
      const key = new Uint8Array(32);
      const encrypted = new Uint8Array(128);
      const decrypted = data;

      when(mockCrypto.encryptSymmetric(key, data))
        .resolves(encrypted);
      when(mockCrypto.decryptSymmetric(key, encrypted))
        .resolves(decrypted);

      const ciphertext = await client.encryptData(key, data);
      const plaintext = await client.decryptData(key, ciphertext);
      expect(plaintext).to.eql(data);
    });

    it('should sign and verify messages', async () => {
      const sig = new Uint8Array(64);
      when(mockCrypto.sign(keyPair.secretKey, data))
        .resolves(sig);
      when(mockCrypto.verify(keyPair.publicKey, data, sig))
        .resolves(true);

      const signature = await client.signData(keyPair.secretKey, data);
      const isValid = await client.verifySignature(
        keyPair.publicKey, 
        data, 
        signature
      );
      expect(isValid).to.be.true;
    });
  });

  describe('Error Handling', () => {
    it('should wrap native errors in HauntiError', async () => {
      when(mockProgram.methods.getTaskStatus(any))
        .throws(new Error('Network error'));

      await expect(client.getTaskStatus('invalid_id'))
        .to.be.rejectedWith(HauntiError)
        .with.property('code', 'TASK_FETCH_FAILED');
    });

    it('should handle edge cases in reward calculation', async () => {
      when(mockProgram.account.stakingPool.fetch(any))
        .resolves({ apy: 0 }); // Zero edge case

      const apy = await client.getPoolAPY(StakingPoolType.Validator);
      expect(apy).to.equal(0);
    });
  });
});
