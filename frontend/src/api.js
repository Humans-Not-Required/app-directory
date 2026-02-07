const BASE = '/api/v1';

function getKey() {
  return localStorage.getItem('appdir_api_key') || '';
}

function setKey(key) {
  localStorage.setItem('appdir_api_key', key);
}

async function request(path, opts = {}) {
  const key = getKey();
  const headers = { ...(opts.headers || {}) };
  // Only add Authorization header if we have a key (not required for most endpoints now)
  if (key && key.trim()) headers['Authorization'] = `Bearer ${key}`;
  if (opts.body && typeof opts.body === 'object') {
    headers['Content-Type'] = 'application/json';
    opts.body = JSON.stringify(opts.body);
  }
  const res = await fetch(`${BASE}${path}`, { ...opts, headers });
  const rateLimit = {
    limit: res.headers.get('X-RateLimit-Limit'),
    remaining: res.headers.get('X-RateLimit-Remaining'),
    reset: res.headers.get('X-RateLimit-Reset'),
  };
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    throw { status: res.status, ...err, rateLimit };
  }
  const data = await res.json().catch(() => null);
  return { data, rateLimit };
}

// Apps
const listApps = (params = '') => request(`/apps${params ? '?' + params : ''}`);
const searchApps = (q, params = '') =>
  request(`/apps/search?q=${encodeURIComponent(q)}${params ? '&' + params : ''}`);
const getApp = (id) => request(`/apps/${id}`);
const submitApp = (body) => request('/apps', { method: 'POST', body });
const updateApp = (id, body) => request(`/apps/${id}`, { method: 'PATCH', body });
const deleteApp = (id) => request(`/apps/${id}`, { method: 'DELETE' });

// Reviews
const getReviews = (appId, params = '') =>
  request(`/apps/${appId}/reviews${params ? '?' + params : ''}`);
const submitReview = (appId, body) =>
  request(`/apps/${appId}/reviews`, { method: 'POST', body });

// Categories
const listCategories = () => request('/categories');

// Admin: approval
const listPending = (params = '') => request(`/apps/pending${params ? '?' + params : ''}`);
const approveApp = (id, note) =>
  request(`/apps/${id}/approve`, { method: 'POST', body: note ? { note } : {} });
const rejectApp = (id, reason) =>
  request(`/apps/${id}/reject`, { method: 'POST', body: { reason } });

// Admin: deprecation
const deprecateApp = (id, body) =>
  request(`/apps/${id}/deprecate`, { method: 'POST', body });
const undeprecateApp = (id) =>
  request(`/apps/${id}/undeprecate`, { method: 'POST' });

// Health
const healthSummary = () => request('/apps/health/summary');
const appHealthHistory = (id, params = '') =>
  request(`/apps/${id}/health${params ? '?' + params : ''}`);
const triggerHealthCheck = (id) =>
  request(`/apps/${id}/health-check`, { method: 'POST' });
const batchHealthCheck = () =>
  request('/apps/health-check/batch', { method: 'POST' });

// Stats
const getAppStats = (id) => request(`/apps/${id}/stats`);
const trendingApps = (days = 7, limit = 10) =>
  request(`/apps/trending?days=${days}&limit=${limit}`);

// Keys (admin)
const listKeys = () => request('/keys');
const createKey = (body) => request('/keys', { method: 'POST', body });
const deleteKey = (id) => request(`/keys/${id}`, { method: 'DELETE' });

// Health check
const health = () => request('/health');

export {
  getKey, setKey,
  listApps, searchApps, getApp, submitApp, updateApp, deleteApp,
  getReviews, submitReview,
  listCategories,
  listPending, approveApp, rejectApp,
  deprecateApp, undeprecateApp,
  healthSummary, appHealthHistory, triggerHealthCheck, batchHealthCheck,
  getAppStats, trendingApps,
  listKeys, createKey, deleteKey,
  health,
};
