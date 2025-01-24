import express, { Request, Response, Application } from 'express';
import { ArbitrageBot } from './bot';
import dotenv from 'dotenv';
import { startBot } from './bot';
import { Server } from 'http';

dotenv.config();

const app = express();
const bot = new ArbitrageBot();
let server: Server | null = null;

app.get('/status', (req: Request, res: Response) => {
    res.json({ 
        status: 'Running', 
        timestamp: new Date() 
    });
});

// Try different ports if 3000 is in use
const ports = [3000, 3001, 3002, 3003];

app.get('/health', (req: Request, res: Response) => {
    res.json({ 
        status: 'healthy',
        botStatus: bot.getStatus(),
        uptime: process.uptime(),
        memory: process.memoryUsage()
    });
});

// Add shutdown endpoint
app.post('/shutdown', (req: Request, res: Response) => {
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
        await startBot();
        
        // Try each port until one works
        for (const port of ports) {
            try {
                await new Promise<Server | null>((resolve, reject) => {
                    server = app.listen(port)
                        .once('listening', () => {
                            console.log(`Server running on port ${port}`);
                            resolve(server);
                        })
                        .once('error', (err: NodeJS.ErrnoException) => {
                            if (err.code === 'EADDRINUSE') {
                                console.log(`Port ${port} is busy, trying next port...`);
                                resolve(null);
                            } else {
                                reject(err);
                            }
                        });
                });
                break; // If we get here, the server started successfully
            } catch (err) {
                console.error(`Error starting server on port ${port}:`, err);
            }
        }
    } catch (error) {
        console.error('Error starting bot:', error);
        process.exit(1);
    }
}

main();