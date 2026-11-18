import { createRoot } from 'react-dom/client';

const root = createRoot(document.getElementById('root'));
const route = location.pathname.replace(/\/$/, '');

async function renderPage() {
  switch (route) {
    case '/admin': {
      const { default: AdminApp } = await import('./admin');
      document.title = 'Server Administration';
      root.render(<AdminApp />);
      break;
    }
    case '/shop': {
      const { default: Shop } = await import('./shop');
      document.title = 'Garage Store';
      root.render(<Shop />);
      break;
    }
    case '':
      location.replace('/admin/' + location.hash);
      break;
    default:
      root.render(<p>Page unavailable.</p>);
  }
}

renderPage().catch(() => root.render(<p>Could not load the page. Reload to try again.</p>));
