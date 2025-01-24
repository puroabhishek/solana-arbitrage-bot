import dotenv from 'dotenv';
import { PublicKey } from '@solana/web3.js';

dotenv.config();

export const CONFIG = {
    RPC_URL: process.env.SOLANA_RPC_URL || '',
    PRIVATE_KEY: process.env.PRIVATE_KEY || '',
    MIN_PROFIT_PERCENTAGE: 1.5,
    SUPPORTED_DEXES: {
        RAYDIUM: {
            programId: new PublicKey('675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8')
        }
    }
};

export const config = {
    // Add your configuration variables here
    // Example:
    // apiKey: process.env.API_KEY,
};