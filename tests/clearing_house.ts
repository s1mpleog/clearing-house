import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { ClearingHouse } from "../target/types/clearing_house";
import {
  createAssociatedTokenAccount,
  createMint,
  getAccount,
  getAssociatedTokenAddress,
  getOrCreateAssociatedTokenAccount,
  mintTo,
} from "@solana/spl-token";
import { assert } from "chai";
import { PublicKey } from "@solana/web3.js";

describe("clearing_house", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const connection = provider.connection;
  const payer = (provider.wallet as anchor.Wallet).payer;
  const program = anchor.workspace.clearingHouse as Program<ClearingHouse>;

  let mintA: PublicKey;
  let mintB: PublicKey;
  let market: PublicKey;
  let vaultA: PublicKey;
  let vaultB: PublicKey;

  async function fundAccount(keypair: anchor.web3.Keypair, lamports = 20) {
    await connection.requestAirdrop(keypair.publicKey, lamports * anchor.web3.LAMPORTS_PER_SOL);
    await new Promise((r) => setTimeout(r, 1500));
  }

  before(async () => {
    mintA = await createMint(connection, payer, payer.publicKey, null, 6);
    mintB = await createMint(connection, payer, payer.publicKey, null, 6);

    if (mintA.toBuffer().compare(mintB.toBuffer()) > 0) {
      [mintA, mintB] = [mintB, mintA];
    }

    [market] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("market"), mintA.toBuffer(), mintB.toBuffer()],
      program.programId
    );

    vaultA = await getAssociatedTokenAddress(mintA, market, true);
    vaultB = await getAssociatedTokenAddress(mintB, market, true);

    await program.methods
      .initializeMarket()
      .accounts({
        payer: payer.publicKey,
        mintA,
        mintB,
        market,
        vaultA,
        vaultB,
        tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();
  });

  describe("user position & orders", () => {
    let buyer: anchor.web3.Keypair;
    let buyerPosition: PublicKey;
    let buyerAtaA: PublicKey;
    let buyerAtaB: PublicKey;

    let seller: anchor.web3.Keypair;
    let sellerPosition: PublicKey;
    let sellerAtaA: PublicKey;
    let sellerAtaB: PublicKey;

    beforeEach(async () => {
      buyer = anchor.web3.Keypair.generate();
      seller = anchor.web3.Keypair.generate();

      await fundAccount(buyer);
      await fundAccount(seller);

      buyerAtaA = await getOrCreateAssociatedTokenAccount(
        connection,
        payer,
        mintA,
        buyer.publicKey,
        true
      ).then(acc => acc.address);

      buyerAtaB = await getOrCreateAssociatedTokenAccount(
        connection,
        payer,
        mintB,
        buyer.publicKey,
        true
      ).then(acc => acc.address);

      sellerAtaA = await getOrCreateAssociatedTokenAccount(
        connection,
        payer,
        mintA,
        seller.publicKey,
        true
      ).then(acc => acc.address);

      sellerAtaB = await getOrCreateAssociatedTokenAccount(
        connection,
        payer,
        mintB,
        seller.publicKey,
        true
      ).then(acc => acc.address);

      await mintTo(connection, payer, mintA, buyerAtaA, payer, 50_000_000_000);
      await mintTo(connection, payer, mintB, buyerAtaB, payer, 50_000_000_000);
      await mintTo(connection, payer, mintA, sellerAtaA, payer, 50_000_000_000);
      await mintTo(connection, payer, mintB, sellerAtaB, payer, 50_000_000_000);

      [buyerPosition] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("user_position"), buyer.publicKey.toBuffer(), market.toBuffer()],
        program.programId
      );

      [sellerPosition] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("user_position"), seller.publicKey.toBuffer(), market.toBuffer()],
        program.programId
      );

      await program.methods
        .initializeUser()
        .accounts({
          user: buyer.publicKey,
          mintA,
          mintB,
          market,
          userPosition: buyerPosition,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([buyer])
        .rpc();

      await program.methods
        .initializeUser()
        .accounts({
          user: seller.publicKey,
          mintA,
          mintB,
          market,
          userPosition: sellerPosition,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([seller])
        .rpc();
    });

    it("places sell order and can cancel it", async () => {
      const [sellOrderPda] = anchor.web3.PublicKey.findProgramAddressSync(
        [
          Buffer.from("order"),
          seller.publicKey.toBuffer(),
          market.toBuffer(),
          Buffer.from(new anchor.BN(0).toArray("le", 8)),
        ],
        program.programId
      );

      await program.methods
        .placeOrder({ sell: {} }, new anchor.BN(100), new anchor.BN(100_000_000))
        .accounts({
          user: seller.publicKey,
          mintA,
          mintB,
          market,
          userPosition: sellerPosition,
          order: sellOrderPda,
          vaultA,
          vaultB,
          userTokenAccountA: sellerAtaA,
          userTokenAccountB: sellerAtaB,
          systemProgram: anchor.web3.SystemProgram.programId,
          tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        })
        .signers([seller])
        .rpc();

      const vaultAInfo = await getAccount(connection, vaultA);
      assert.equal(Number(vaultAInfo.amount), 100_000_000);

      await program.methods
        .cancelOrder()
        .accounts({
          user: seller.publicKey,
          mintA,
          mintB,
          market,
          vaultA,
          vaultB,
          userTokenAccountA: sellerAtaA,
          userTokenAccountB: sellerAtaB,
          order: sellOrderPda,
          userPosition: sellerPosition,
          tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        })
        .signers([seller])
        .rpc();

      const sellerAtaAInfo = await getAccount(connection, sellerAtaA);
      assert.equal(Number(sellerAtaAInfo.amount), 50_000_000_000);

      const order = await program.account.order.fetch(sellOrderPda);
      assert.equal(order.status.cancelled !== undefined, true);

      const pos = await program.account.userPosition.fetch(sellerPosition);
      assert.equal(pos.openOrders.toNumber(), 0);
    });

    it("places buy order and can cancel it", async () => {
      const [buyOrderPda] = anchor.web3.PublicKey.findProgramAddressSync(
        [
          Buffer.from("order"),
          buyer.publicKey.toBuffer(),
          market.toBuffer(),
          Buffer.from(new anchor.BN(0).toArray("le", 8)),
        ],
        program.programId
      );

      await program.methods
        .placeOrder({ buy: {} }, new anchor.BN(100), new anchor.BN(100_000_000))
        .accounts({
          user: buyer.publicKey,
          mintA,
          mintB,
          market,
          userPosition: buyerPosition,
          order: buyOrderPda,
          vaultA,
          vaultB,
          userTokenAccountA: buyerAtaA,
          userTokenAccountB: buyerAtaB,
          systemProgram: anchor.web3.SystemProgram.programId,
          tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        })
        .signers([buyer])
        .rpc();

      const vaultBBefore = await getAccount(connection, vaultB);
      assert.equal(Number(vaultBBefore.amount), 10_000_000_000);

      await program.methods
        .cancelOrder()
        .accounts({
          user: buyer.publicKey,
          mintA,
          mintB,
          market,
          vaultA,
          vaultB,
          userTokenAccountA: buyerAtaA,
          userTokenAccountB: buyerAtaB,
          order: buyOrderPda,
          userPosition: buyerPosition,
          tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        })
        .signers([buyer])
        .rpc();

      const buyerAtaBBefore = await getAccount(connection, buyerAtaB);
      assert.equal(Number(buyerAtaBBefore.amount), 50_000_000_000);

      const order = await program.account.order.fetch(buyOrderPda);
      assert.equal(order.status.cancelled !== undefined, true);

      const pos = await program.account.userPosition.fetch(buyerPosition);
      assert.equal(pos.openOrders.toNumber(), 0);
    });

  });
});
