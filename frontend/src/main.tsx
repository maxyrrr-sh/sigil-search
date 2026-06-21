import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { EuiProvider } from '@elastic/eui';

import App from './App';

type ColorMode = 'light' | 'dark';

function Root() {
  const [colorMode, setColorMode] = useState<ColorMode>(
    () => (localStorage.getItem('sigil.theme') as ColorMode) || 'light',
  );

  const toggle = () => {
    setColorMode((prev) => {
      const next: ColorMode = prev === 'light' ? 'dark' : 'light';
      localStorage.setItem('sigil.theme', next);
      return next;
    });
  };

  return (
    <EuiProvider colorMode={colorMode}>
      <App colorMode={colorMode} onToggleTheme={toggle} />
    </EuiProvider>
  );
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
