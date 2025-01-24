"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.config = exports.CONFIG = void 0;
const dotenv_1 = __importDefault(require("dotenv"));
const web3_js_1 = require("@solana/web3.js");
dotenv_1.default.config();
exports.CONFIG = {
    RPC_URL: process.env.SOLANA_RPC_URL || '',
    PRIVATE_KEY: process.env.PRIVATE_KEY || '',
    MIN_PROFIT_PERCENTAGE: 1.5,
    SUPPORTED_DEXES: {
        RAYDIUM: {
            programId: new web3_js_1.PublicKey('675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8')
        }
    }
};
exports.config = {
// Add your configuration variables here
// Example:
// apiKey: process.env.API_KEY,
};
