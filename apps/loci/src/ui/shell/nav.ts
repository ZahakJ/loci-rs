import type { IconName } from '../design/icons';

export interface NavItem { id: 'home' | 'browse' | 'stats' | 'settings'; href: string; icon: IconName; label: string; kbd?: string }
export const NAV: NavItem[] = [
  { id: 'home', href: '#/', icon: 'home', label: 'nav.home', kbd: 'd' },
  { id: 'browse', href: '#/browse', icon: 'browse', label: 'nav.browse', kbd: 'b' },
  { id: 'stats', href: '#/stats/all', icon: 'stats', label: 'nav.stats', kbd: 's' },
  { id: 'settings', href: '#/settings', icon: 'settings', label: 'nav.settings' },
];
