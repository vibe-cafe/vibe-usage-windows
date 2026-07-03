#!/usr/bin/env node

/**
 * vibe-usage CLI entry point.
 * Routes to the appropriate command handler.
 */

import { run } from '../src/index.js';

run(process.argv.slice(2));
