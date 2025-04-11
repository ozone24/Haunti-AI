import { 
  Connection, 
  Keypair, 
  PublicKey, 
  Transaction, 
  TransactionInstruction,
  SystemProgram,
  sendAndConfirmTransaction,
  Signer,
  Commitment,
  RpcResponseAndContext,
  SignatureResult,
  TokenBalance,
  ParsedAccountData,
} from '@solana/web3.js';
import { 
  Program, 
  AnchorProvider, 
  Wallet, 
  BN 
} from '@project-serum/anchor';
import { 
  ASSOCIATED_TOKEN_PROGRAM_ID, 
  TOKEN_PROGRAM_ID, 
  createAssociatedTokenAccountInstruction,
  getAssociatedTokenAddress,
} from '@solana/spl-token';
import { IDL as HauntiIDL } from './haunti_program';
import { Haunti } from './types';
import { 
  WalletError, 
  WalletNotConnectedError,
  TransactionError,
  ProgramError,
  NetworkError,
} from './errors';

const HAUNTI_PROGRAM_ID = new PublicKey(process.env.NEXT_PUBLIC_PROGRAM_ID!);
const HAUNTI_TOKEN_MINT = new PublicKey(process.env.NEXT_PUBLIC_TOKEN_MINT!);

export interface NetworkConfig {
  rpcEndpoint: string;
  wsEndpoint?: string;
  commitment: Commitment;
}

export const NETWORKS: Record<string, NetworkConfig> = {
  mainnet: {
    rpcEndpoint: "https://api.mainnet-beta.solana.com",
    commitment: "confirmed"
  },
  devnet: {
    rpcEndpoint: "https://api.devnet.solana.com",
    commitment: "processed"
  },
  localnet: {
    rpcEndpoint: "http://127.0.0.1:8899",
    commitment: "finalized"
  }
};

export async function initializeConnection(
  network: string = 'mainnet'
): Promise<Connection> {
  try {
    const config = NETWORKS[network] || NETWORKS.mainnet;
    const connection = new Connection(
      config.rpcEndpoint, 
      {
        commitment: config.commitment,
        wsEndpoint: config.wsEndpoint
      }
    );
    await connection.getVersion();
    return connection;
  } catch (error) {
    throw new NetworkError(`Failed to initialize connection: ${error.message}`);
  }
}

export function createProgram(
  connection: Connection,
  wallet: Wallet
): Program<Haunti> {
  return new Program<Haunti>(
    HauntiIDL,
    HAUNTI_PROGRAM_ID,
    new AnchorProvider(
      connection,
      wallet,
      { commitment: connection.commitment }
    )
  );
}

export async function createTransaction(
  connection: Connection,
  feePayer: PublicKey,
  instructions: TransactionInstruction[],
  signers: Signer[] = []
): Promise<Transaction> {
  const transaction = new Transaction().add(...instructions);
  transaction.feePayer = feePayer;
  transaction.recentBlockhash = (
    await connection.getLatestBlockhash()
  ).blockhash;
  
  if (signers.length > 0) {
    transaction.sign(...signers);
  }
  
  if (!transaction.verifySignatures()) {
    throw new TransactionError("Transaction signature verification failed");
  }
  
  return transaction;
}

export async function sendTransaction(
  connection: Connection,
  transaction: Transaction,
  signers: Signer[] = []
): Promise<string> {
  try {
    return await sendAndConfirmTransaction(
      connection,
      transaction,
      signers,
      {
        skipPreflight: false,
        commitment: connection.commitment,
      }
    );
  } catch (error) {
    throw new TransactionError(`Transaction failed: ${error.message}`);
  }
}

export async function getTokenBalance(
  connection: Connection,
  wallet: PublicKey
): Promise<number> {
  try {
    const tokenAccount = await getAssociatedTokenAddress(
      HAUNTI_TOKEN_MINT,
      wallet
    );
    
    const balance = await connection.getTokenAccountBalance(tokenAccount);
    return balance.value.uiAmount || 0;
  } catch (error) {
    if (error.message.includes("Account not found")) return 0;
    throw new WalletError(`Balance check failed: ${error.message}`);
  }
}

export async function transferTokens(
  connection: Connection,
  sender: Keypair,
  recipient: PublicKey,
  amount: number
): Promise<string> {
  const fromTokenAccount = await getAssociatedTokenAddress(
    HAUNTI_TOKEN_MINT,
    sender.publicKey
  );
  
  const toTokenAccount = await getAssociatedTokenAddress(
    HAUNTI_TOKEN_MINT,
    recipient
  );

  const instructions = [];
  
  // Create recipient token account if needed
  if (!await connection.getAccountInfo(toTokenAccount)) {
    instructions.push(
      createAssociatedTokenAccountInstruction(
        sender.publicKey,
        toTokenAccount,
        recipient,
        HAUNTI_TOKEN_MINT
      )
    );
  }

  // Transfer tokens
  instructions.push(
    Token.createTransferInstruction(
      TOKEN_PROGRAM_ID,
      fromTokenAccount,
      toTokenAccount,
      sender.publicKey,
      [],
      amount
    )
  );

  const transaction = await createTransaction(
    connection,
    sender.publicKey,
    instructions,
    [sender]
  );

  return sendTransaction(connection, transaction, [sender]);
}

export async function submitComputeTask(
  program: Program<Haunti>,
  wallet: Wallet,
  taskType: number,
  modelCID: string,
  resources: {
    gpuType: number;
    minMemory: number;
    timeout: number;
  }
): Promise<string> {
  try {
    const [taskAccount] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("compute_task"),
        wallet.publicKey.toBuffer(),
        new BN(Date.now()).toArrayLike(Buffer, "le", 8),
      ],
      HAUNTI_PROGRAM_ID
    );

    const tx = await program.methods
      .submitTask({
        taskType,
        modelCid: modelCID,
        gpuType: resources.gpuType,
        minMemory: new BN(resources.minMemory),
        timeout: new BN(resources.timeout),
      })
      .accounts({
        taskAccount,
        user: wallet.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .transaction();

    return await sendTransaction(program.provider.connection, tx, [wallet.payer]);
  } catch (error) {
    if (error?.code) {
      throw new ProgramError(error.code, error.msg);
    }
    throw new TransactionError(`Task submission failed: ${error.message}`);
  }
}

export async function getTaskStatus(
  program: Program<Haunti>,
  taskAccount: PublicKey
): Promise<{
  state: number;
  resultCID: string;
  resourcesUsed: {
    gpuMem: BN;
    computeUnits: BN;
  };
}> {
  try {
    const account = await program.account.computeTask.fetch(taskAccount);
    return {
      state: account.state,
      resultCID: account.resultCid,
      resourcesUsed: {
        gpuMem: account.gpuMemUsed,
        computeUnits: account.computeUnits,
      }
    };
  } catch (error) {
    if (error.message.includes("Account does not exist")) {
      throw new ProgramError(404, "Task account not found");
    }
    throw new NetworkError(`Failed to fetch task status: ${error.message}`);
  }
}

export function validateSolanaAddress(address: string): boolean {
  try {
    new PublicKey(address);
    return true;
  } catch {
    return false;
  }
}

export async function awaitTransactionConfirmation(
  connection: Connection,
  signature: string,
  timeout: number = 60000
): Promise<RpcResponseAndContext<SignatureResult>> {
  const start = Date.now();
  
  while (Date.now() - start < timeout) {
    const result = await connection.getSignatureStatus(signature);
    
    if (result?.value?.confirmationStatus === 'finalized') {
      return result;
    }
    
    await new Promise(resolve => setTimeout(resolve, 2000));
  }
  
  throw new TransactionError("Transaction confirmation timeout");
}

// Additional utility types
export interface ComputeTaskConfig {
  taskType: number;
  modelCID: string;
  gpuType: number;
  minMemory: number;
  timeout: number;
}

export type TokenBalanceResponse = {
  balance: number;
  decimals: number;
  uiAmount: number;
};

export type TransactionResult = {
  signature: string;
  slot: number;
  confirmation: Commitment;
};
