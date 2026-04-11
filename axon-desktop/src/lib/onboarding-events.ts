type Listener = () => void;
let openListener: Listener | null = null;

export const onboardingEvents = {
  open:   ()                => openListener?.(),
  onOpen: (l: Listener)    => { openListener = l; return () => { openListener = null; }; },
};

const KEY = 'axon_onboarded';
export const markOnboarded  = () => { try { localStorage.setItem(KEY, '1'); } catch { /* */ } };
export const isOnboarded    = () => { try { return !!localStorage.getItem(KEY); } catch { return false; } };
export const resetOnboarded = () => { try { localStorage.removeItem(KEY); } catch { /* */ } };
