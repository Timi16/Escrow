import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, Keypair, LAMPORTS_PER_SOL } from '@solana/web3.js';
import fetch from 'node-fetch';

// Tensor API endpoints
const TENSOR_API_ENDPOINT = 'https://api.tensor.so/graphql';

async function getTensorFloorPrice(slug: string): Promise<number> {
  const query = `{
    collectionStats(slug: "${slug}") {
      floor
    }
  }`;

  const response = await fetch(TENSOR_API_ENDPOINT, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ query })
  });

  const data = await response.json();
  return data.data.collectionStats.floor;
}

const IDL = {
  version: "0.1.0",
  name: "escrowfloor",
  metadata: {
    address: "4gjmWmuanYNZTsU1vXnUSUsphL9BYBNSkh6UoU5ym9i4"
  },
  instructions: [
    {
      name: "initializeEscrow",
      discriminator: [0],
      accounts: [
        { name: "trader", writable: true, signer: true },
        { name: "escrow", writable: true, signer: false },
        { name: "tensorOracle", writable: false, signer: false },
        { name: "systemProgram", writable: false, signer: false }
      ],
      args: [
        { name: "collectionId", type: "string" },
        { name: "predictedFloor", type: "u64" },
        { name: "expiryTimestamp", type: "i64" },
        { name: "marginAmount", type: "u64" }
      ]
    },
    {
      name: "acceptEscrow",
      discriminator: [1],
      accounts: [
        { name: "trader", writable: true, signer: true },
        { name: "escrow", writable: true, signer: false },
        { name: "systemProgram", writable: false, signer: false }
      ],
      args: []
    },
    {
      name: "settleEscrow",
      discriminator: [2],
      accounts: [
        { name: "winner", writable: true, signer: false },
        { name: "escrow", writable: true, signer: false },
        { name: "tensorOracle", writable: false, signer: false },
        { name: "systemProgram", writable: false, signer: false }
      ],
      args: []
    }
  ]
};

describe("escrowfloor", () => {
  // Configure the client to use devnet
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.Escrowfloor;
  
  // Tensor swap program ID
  const TENSOR_SWAP_ID = new PublicKey("TSWAPaqyCSx2KABk68Shruf4rp7CxcNi8hAsbdwmHbN");

  // Test collection - y00ts
  const COLLECTION_SLUG = "y00ts";

  it("Full escrow flow with real Tensor price", async () => {
    // Get current floor price
    const currentFloor = await getTensorFloorPrice(COLLECTION_SLUG);
    console.log(`Current floor price: ${currentFloor} SOL`);

    // Create traders
    const trader1 = Keypair.generate();
    const trader2 = Keypair.generate();

    // Fund traders
    await provider.connection.requestAirdrop(trader1.publicKey, 2 * LAMPORTS_PER_SOL);
    await provider.connection.requestAirdrop(trader2.publicKey, 2 * LAMPORTS_PER_SOL);

    // Predict 10% higher floor
    const predictedFloor = Math.floor(currentFloor * 1.1 * LAMPORTS_PER_SOL);
    const marginAmount = new anchor.BN(0.5 * LAMPORTS_PER_SOL); // 0.5 SOL margin

    // Create escrow PDA
    const [escrowPDA] = PublicKey.findProgramAddressSync(
      [Buffer.from("escrow"), trader1.publicKey.toBuffer()],
      program.programId
    );

    console.log("Creating escrow...");
    const tx1 = await program.methods
      .initializeEscrow(
        COLLECTION_SLUG,
        new anchor.BN(predictedFloor),
        new anchor.BN(Date.now()/1000 + 3600), // 1 hour expiry
        marginAmount
      )
      .accounts({
        trader: trader1.publicKey,
        escrow: escrowPDA,
        tensorOracle: TENSOR_SWAP_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([trader1])
      .rpc();

    console.log("Escrow created:", tx1);

    console.log("Accepting escrow...");
    const tx2 = await program.methods
      .acceptEscrow()
      .accounts({
        trader: trader2.publicKey,
        escrow: escrowPDA,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([trader2])
      .rpc();

    console.log("Escrow accepted:", tx2);

    // For testing, we'll settle immediately instead of waiting an hour
    console.log("Settling escrow...");
    const tx3 = await program.methods
      .settleEscrow()
      .accounts({
        winner: trader1.publicKey, // Will be determined by program
        escrow: escrowPDA,
        tensorOracle: TENSOR_SWAP_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    console.log("Escrow settled:", tx3);
  });
});
