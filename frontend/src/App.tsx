import { useEffect, useState } from 'react';
import {
  EuiButtonEmpty,
  EuiHeader,
  EuiHeaderLogo,
  EuiHeaderSection,
  EuiHeaderSectionItem,
  EuiHealth,
} from '@elastic/eui';

import { health } from './api/client';
import { DashboardsView } from './components/DashboardsView';
import { SearchView } from './components/SearchView';
import { ThemeToggle } from './components/ThemeToggle';

type View = 'search' | 'dashboards';

export default function App({ colorMode, onToggleTheme }: { colorMode: 'light' | 'dark'; onToggleTheme: () => void }) {
  const [view, setView] = useState<View>('search');
  const [online, setOnline] = useState<boolean | null>(null);

  useEffect(() => {
    health().then(setOnline);
  }, []);

  return (
    <>
      <EuiHeader>
        <EuiHeaderSection>
          <EuiHeaderSectionItem>
            <EuiHeaderLogo iconType="discoverApp">Sigil&nbsp;Search</EuiHeaderLogo>
          </EuiHeaderSectionItem>
          <EuiHeaderSectionItem>
            <EuiButtonEmpty
              data-test-subj="navSearch"
              color={view === 'search' ? 'primary' : 'text'}
              onClick={() => setView('search')}
            >
              Search
            </EuiButtonEmpty>
          </EuiHeaderSectionItem>
          <EuiHeaderSectionItem>
            <EuiButtonEmpty
              data-test-subj="navDashboards"
              color={view === 'dashboards' ? 'primary' : 'text'}
              onClick={() => setView('dashboards')}
            >
              Dashboards
            </EuiButtonEmpty>
          </EuiHeaderSectionItem>
        </EuiHeaderSection>
        <EuiHeaderSection side="right">
          <EuiHeaderSectionItem>
            <EuiHealth
              color={online === false ? 'danger' : online === null ? 'subdued' : 'success'}
              style={{ paddingRight: 12 }}
            >
              {online === false ? 'API offline' : online === null ? 'connecting…' : 'API connected'}
            </EuiHealth>
          </EuiHeaderSectionItem>
          <EuiHeaderSectionItem>
            <ThemeToggle colorMode={colorMode} onToggle={onToggleTheme} />
          </EuiHeaderSectionItem>
        </EuiHeaderSection>
      </EuiHeader>

      <div style={{ padding: 16, maxWidth: 1600, margin: '0 auto' }}>
        {view === 'search' ? <SearchView /> : <DashboardsView />}
      </div>
    </>
  );
}
