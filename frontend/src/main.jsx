import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App.jsx';

document.body.style.margin = '0';
document.body.style.background = '#0f172a';

createRoot(document.getElementById('root')).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
