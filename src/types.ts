export interface PriceData {
    dex: string;
    tokenPair: string;
    price: number;
    timestamp: number;
}

export interface ArbitrageOpportunity {
    buyDex: string;
    sellDex: string;
    tokenPair: string;
    profitPercentage: number;
}

export interface Config {
    // Add your config type definitions
}