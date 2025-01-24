"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const express_1 = __importDefault(require("express"));
const bot_1 = require("./bot");
const dotenv_1 = __importDefault(require("dotenv"));
const bot_2 = require("./bot");
dotenv_1.default.config();
const app = (0, express_1.default)();
const bot = new bot_1.ArbitrageBot();
let server = null;
app.get('/status', (req, res) => {
    res.json({
        status: 'Running',
        timestamp: new Date()
    });
});
// Try different ports if 3000 is in use
const ports = [3000, 3001, 3002, 3003];
app.get('/health', (req, res) => {
    res.json({
        status: 'healthy',
        botStatus: bot.getStatus(),
        uptime: process.uptime(),
        memory: process.memoryUsage()
    });
});
// Add shutdown endpoint
app.post('/shutdown', (req, res) => {
    res.json({ message: 'Shutting down...' });
    console.log('Shutdown requested, closing server...');
    // Graceful shutdown
    setTimeout(() => {
        server?.close(() => {
            console.log('Server closed');
            process.exit(0);
        });
    }, 1000);
});
async function main() {
    try {
        await (0, bot_2.startBot)();
        // Try each port until one works
        for (const port of ports) {
            try {
                await new Promise((resolve, reject) => {
                    server = app.listen(port)
                        .once('listening', () => {
                        console.log(`Server running on port ${port}`);
                        resolve(server);
                    })
                        .once('error', (err) => {
                        if (err.code === 'EADDRINUSE') {
                            console.log(`Port ${port} is busy, trying next port...`);
                            resolve(null);
                        }
                        else {
                            reject(err);
                        }
                    });
                });
                break; // If we get here, the server started successfully
            }
            catch (err) {
                console.error(`Error starting server on port ${port}:`, err);
            }
        }
    }
    catch (error) {
        console.error('Error starting bot:', error);
        process.exit(1);
    }
}
main();
