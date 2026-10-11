import pkg from '../../../package.json';
export const APP_VERSION: string = (pkg as { version: string }).version;
export const REPO_URL = 'https://github.com/ZahakJ/loci';
export const LICENSE: string = (pkg as { license?: string }).license ?? 'MIT';
