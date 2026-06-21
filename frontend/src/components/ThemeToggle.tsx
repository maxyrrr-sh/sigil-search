import { EuiButtonIcon } from '@elastic/eui';

export function ThemeToggle({ colorMode, onToggle }: { colorMode: 'light' | 'dark'; onToggle: () => void }) {
  return (
    <EuiButtonIcon
      iconType={colorMode === 'dark' ? 'sun' : 'moon'}
      aria-label={colorMode === 'dark' ? 'Switch to light theme' : 'Switch to dark theme'}
      onClick={onToggle}
      display="base"
      size="m"
    />
  );
}
