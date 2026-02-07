import { useState, useEffect, useCallback } from 'react';
import * as api from './api';

const PROTOCOLS = ['rest', 'graphql', 'grpc', 'mcp', 'a2a', 'websocket', 'other'];
const CATEGORIES = [
  'ai-model', 'automation', 'code-tools', 'communication',
  'data', 'devops', 'finance', 'monitoring',
  'productivity', 'search', 'security', 'other',
];

const PROTOCOL_COLORS = {
  rest: '#3b82f6', graphql: '#e535ab', grpc: '#00b4ab', mcp: '#8b5cf6',
  a2a: '#f59e0b', websocket: '#10b981', other: '#6b7280',
};

const HEALTH_COLORS = {
  healthy: '#22c55e', unhealthy: '#ef4444', unreachable: '#f59e0b', unknown: '#6b7280',
};

const PRIORITY_STARS = (rating) => {
  const full = Math.floor(rating);
  const half = rating - full >= 0.5;
  return '★'.repeat(full) + (half ? '½' : '') + '☆'.repeat(5 - full - (half ? 1 : 0));
};

function ApiKeyPrompt({ onSave }) {
  const [key, setKey] = useState('');
  return (
    <div style={styles.overlay}>
      <div style={styles.modal}>
        <h2 style={{ margin: '0 0 8px' }}>🔑 API Key Required</h2>
        <p style={{ color: '#94a3b8', margin: '0 0 16px', fontSize: 14 }}>
          Enter your API key to access the App Directory.
        </p>
        <input
          style={styles.input}
          placeholder="ad_..."
          value={key}
          onChange={(e) => setKey(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && key.trim() && onSave(key.trim())}
        />
        <button
          style={{ ...styles.btn, ...styles.btnPrimary, width: '100%', marginTop: 8 }}
          onClick={() => key.trim() && onSave(key.trim())}
        >
          Connect
        </button>
      </div>
    </div>
  );
}

function Badge({ label, color, small }) {
  return (
    <span style={{
      display: 'inline-block', padding: small ? '1px 6px' : '2px 8px',
      borderRadius: 9999, fontSize: small ? 10 : 11, fontWeight: 600,
      background: color + '22', color, border: `1px solid ${color}44`,
    }}>
      {label}
    </span>
  );
}

function AppCard({ app, onClick }) {
  return (
    <div style={styles.card} onClick={onClick}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
            <h3 style={{ margin: 0, fontSize: 16, color: '#f1f5f9' }}>{app.name}</h3>
            {app.is_featured && <Badge label="⭐ Featured" color="#f59e0b" small />}
            {app.is_verified && <Badge label="✓ Verified" color="#22c55e" small />}
          </div>
          {app.slug && (
            <span style={{ fontSize: 11, color: '#64748b' }}>/{app.slug}</span>
          )}
        </div>
        <div style={{ display: 'flex', gap: 4, alignItems: 'center', flexShrink: 0 }}>
          <Badge label={app.protocol} color={PROTOCOL_COLORS[app.protocol] || '#6b7280'} />
          {app.last_health_status && (
            <Badge label={app.last_health_status} color={HEALTH_COLORS[app.last_health_status] || '#6b7280'} small />
          )}
        </div>
      </div>
      <p style={{ margin: '8px 0 0', fontSize: 13, color: '#94a3b8', lineHeight: 1.4 }}>
        {app.description?.length > 150 ? app.description.slice(0, 150) + '...' : app.description}
      </p>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginTop: 8 }}>
        <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
          <Badge label={app.category} color="#6366f1" small />
          {app.tags && JSON.parse(app.tags || '[]').slice(0, 3).map((t) => (
            <Badge key={t} label={t} color="#475569" small />
          ))}
        </div>
        <div style={{ fontSize: 12, color: '#f59e0b' }}>
          {app.avg_rating ? `${PRIORITY_STARS(app.avg_rating)} (${app.review_count})` : 'No reviews'}
        </div>
      </div>
    </div>
  );
}

function AppDetail({ app, onBack, onRefresh }) {
  const [reviews, setReviews] = useState([]);
  const [stats, setStats] = useState(null);
  const [reviewForm, setReviewForm] = useState({ rating: 5, comment: '' });
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    api.getReviews(app.id).then((r) => setReviews(r.data?.reviews || [])).catch(() => {});
    api.getAppStats(app.id).then((r) => setStats(r.data)).catch(() => {});
  }, [app.id]);

  const handleReview = async () => {
    setSubmitting(true);
    try {
      await api.submitReview(app.id, reviewForm);
      const r = await api.getReviews(app.id);
      setReviews(r.data?.reviews || []);
      setReviewForm({ rating: 5, comment: '' });
      onRefresh?.();
    } catch (e) {
      alert(e.error || 'Failed to submit review');
    }
    setSubmitting(false);
  };

  const tags = JSON.parse(app.tags || '[]');

  return (
    <div>
      <button style={styles.btnGhost} onClick={onBack}>← Back to listing</button>
      <div style={{ ...styles.card, marginTop: 8 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', flexWrap: 'wrap', gap: 8 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <h2 style={{ margin: 0, color: '#f1f5f9' }}>{app.name}</h2>
              {app.is_featured && <Badge label="⭐ Featured" color="#f59e0b" />}
              {app.is_verified && <Badge label="✓ Verified" color="#22c55e" />}
              <Badge label={app.status} color={app.status === 'approved' ? '#22c55e' : app.status === 'pending' ? '#f59e0b' : '#ef4444'} />
            </div>
            {app.slug && <span style={{ fontSize: 12, color: '#64748b' }}>/{app.slug}</span>}
          </div>
          <div style={{ display: 'flex', gap: 4 }}>
            <Badge label={app.protocol} color={PROTOCOL_COLORS[app.protocol] || '#6b7280'} />
            <Badge label={app.category} color="#6366f1" />
            {app.last_health_status && (
              <Badge label={app.last_health_status} color={HEALTH_COLORS[app.last_health_status] || '#6b7280'} />
            )}
          </div>
        </div>

        <p style={{ margin: '12px 0', color: '#cbd5e1', lineHeight: 1.6 }}>{app.description}</p>

        {app.deprecated_reason && (
          <div style={{ background: '#7f1d1d22', border: '1px solid #ef444444', borderRadius: 8, padding: 12, marginBottom: 12 }}>
            <strong style={{ color: '#ef4444' }}>⚠️ Deprecated:</strong>
            <span style={{ color: '#fca5a5', marginLeft: 6 }}>{app.deprecated_reason}</span>
            {app.sunset_at && <div style={{ fontSize: 12, color: '#94a3b8', marginTop: 4 }}>Sunset: {new Date(app.sunset_at).toLocaleDateString()}</div>}
          </div>
        )}

        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', marginBottom: 12 }}>
          {app.url && <a href={app.url} target="_blank" rel="noreferrer" style={styles.link}>🔗 Homepage</a>}
          {app.api_spec_url && <a href={app.api_spec_url} target="_blank" rel="noreferrer" style={styles.link}>📄 API Spec</a>}
          {app.source_url && <a href={app.source_url} target="_blank" rel="noreferrer" style={styles.link}>💻 Source</a>}
        </div>

        {tags.length > 0 && (
          <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', marginBottom: 12 }}>
            {tags.map((t) => <Badge key={t} label={t} color="#475569" />)}
          </div>
        )}

        <div style={{ display: 'flex', gap: 16, fontSize: 13, color: '#94a3b8', flexWrap: 'wrap' }}>
          <span>Rating: <strong style={{ color: '#f59e0b' }}>{app.avg_rating ? `${app.avg_rating.toFixed(1)} / 5` : 'None'}</strong> ({app.review_count} reviews)</span>
          {app.uptime_pct != null && <span>Uptime: <strong style={{ color: app.uptime_pct >= 95 ? '#22c55e' : '#f59e0b' }}>{app.uptime_pct.toFixed(1)}%</strong></span>}
          {stats && <span>Views: <strong style={{ color: '#818cf8' }}>{stats.total_views}</strong> ({stats.views_24h} today)</span>}
        </div>
      </div>

      {/* Reviews */}
      <h3 style={{ color: '#f1f5f9', marginTop: 20 }}>Reviews ({reviews.length})</h3>
      <div style={{ ...styles.card, marginBottom: 12 }}>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 8 }}>
          <label style={{ color: '#94a3b8', fontSize: 13 }}>Rating:</label>
          {[1, 2, 3, 4, 5].map((n) => (
            <button
              key={n}
              style={{ background: 'none', border: 'none', cursor: 'pointer', fontSize: 20, color: n <= reviewForm.rating ? '#f59e0b' : '#334155' }}
              onClick={() => setReviewForm((f) => ({ ...f, rating: n }))}
            >★</button>
          ))}
        </div>
        <textarea
          style={{ ...styles.input, minHeight: 60, resize: 'vertical' }}
          placeholder="Write a review (optional)..."
          value={reviewForm.comment}
          onChange={(e) => setReviewForm((f) => ({ ...f, comment: e.target.value }))}
        />
        <button
          style={{ ...styles.btn, ...styles.btnPrimary, marginTop: 8 }}
          onClick={handleReview}
          disabled={submitting}
        >
          {submitting ? 'Submitting...' : 'Submit Review'}
        </button>
      </div>
      {reviews.map((r) => (
        <div key={r.id} style={{ ...styles.card, padding: '10px 14px', marginBottom: 6 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span style={{ color: '#f59e0b', fontSize: 14 }}>{'★'.repeat(r.rating)}{'☆'.repeat(5 - r.rating)}</span>
            <span style={{ fontSize: 11, color: '#64748b' }}>{new Date(r.created_at).toLocaleDateString()}</span>
          </div>
          {r.comment && <p style={{ margin: '4px 0 0', fontSize: 13, color: '#cbd5e1' }}>{r.comment}</p>}
        </div>
      ))}
    </div>
  );
}

function SubmitAppForm({ categories, onSubmit, onCancel }) {
  const [form, setForm] = useState({
    name: '', description: '', url: '', api_spec_url: '', source_url: '',
    protocol: 'rest', category: 'other', tags: '',
  });
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async () => {
    if (!form.name.trim() || !form.description.trim()) return alert('Name and description required');
    setSubmitting(true);
    try {
      const body = {
        ...form,
        tags: form.tags ? form.tags.split(',').map((t) => t.trim()).filter(Boolean) : [],
      };
      if (!body.url) delete body.url;
      if (!body.api_spec_url) delete body.api_spec_url;
      if (!body.source_url) delete body.source_url;
      await api.submitApp(body);
      onSubmit?.();
    } catch (e) {
      alert(e.error || 'Submit failed');
    }
    setSubmitting(false);
  };

  const set = (k) => (e) => setForm((f) => ({ ...f, [k]: e.target.value }));

  return (
    <div style={styles.card}>
      <h3 style={{ margin: '0 0 12px', color: '#f1f5f9' }}>Submit New App</h3>
      <div style={styles.formGrid}>
        <div>
          <label style={styles.label}>Name *</label>
          <input style={styles.input} value={form.name} onChange={set('name')} placeholder="My Cool API" />
        </div>
        <div>
          <label style={styles.label}>Protocol *</label>
          <select style={styles.input} value={form.protocol} onChange={set('protocol')}>
            {PROTOCOLS.map((p) => <option key={p} value={p}>{p.toUpperCase()}</option>)}
          </select>
        </div>
        <div>
          <label style={styles.label}>Category *</label>
          <select style={styles.input} value={form.category} onChange={set('category')}>
            {CATEGORIES.map((c) => <option key={c} value={c}>{c}</option>)}
          </select>
        </div>
        <div>
          <label style={styles.label}>Tags (comma-separated)</label>
          <input style={styles.input} value={form.tags} onChange={set('tags')} placeholder="openai, llm, chat" />
        </div>
        <div style={{ gridColumn: '1 / -1' }}>
          <label style={styles.label}>Description *</label>
          <textarea style={{ ...styles.input, minHeight: 80, resize: 'vertical' }} value={form.description} onChange={set('description')} placeholder="What does this app/service do?" />
        </div>
        <div>
          <label style={styles.label}>Homepage URL</label>
          <input style={styles.input} value={form.url} onChange={set('url')} placeholder="https://..." />
        </div>
        <div>
          <label style={styles.label}>API Spec URL</label>
          <input style={styles.input} value={form.api_spec_url} onChange={set('api_spec_url')} placeholder="https://.../openapi.json" />
        </div>
        <div>
          <label style={styles.label}>Source Code URL</label>
          <input style={styles.input} value={form.source_url} onChange={set('source_url')} placeholder="https://github.com/..." />
        </div>
      </div>
      <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
        <button style={{ ...styles.btn, ...styles.btnPrimary }} onClick={handleSubmit} disabled={submitting}>
          {submitting ? 'Submitting...' : 'Submit App'}
        </button>
        <button style={styles.btnGhost} onClick={onCancel}>Cancel</button>
      </div>
    </div>
  );
}

function AdminPanel({ onRefresh }) {
  const [pending, setPending] = useState([]);
  const [healthSum, setHealthSum] = useState(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [p, h] = await Promise.all([api.listPending(), api.healthSummary()]);
      setPending(p.data?.apps || []);
      setHealthSum(h.data);
    } catch (e) {
      if (e.status === 403) setPending('forbidden');
    }
    setLoading(false);
  }, []);

  useEffect(() => { load(); }, [load]);

  const handleApprove = async (id) => {
    await api.approveApp(id);
    load();
    onRefresh?.();
  };

  const handleReject = async (id) => {
    const reason = prompt('Rejection reason:');
    if (!reason) return;
    await api.rejectApp(id, reason);
    load();
    onRefresh?.();
  };

  const handleBatchHealth = async () => {
    await api.batchHealthCheck();
    load();
  };

  if (loading) return <p style={{ color: '#94a3b8' }}>Loading admin panel...</p>;
  if (pending === 'forbidden') return <p style={{ color: '#f59e0b' }}>Admin key required for this panel.</p>;

  return (
    <div>
      <h3 style={{ color: '#f1f5f9', margin: '0 0 12px' }}>🛡️ Admin Panel</h3>

      {/* Health Summary */}
      {healthSum && (
        <div style={{ ...styles.card, marginBottom: 12 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <h4 style={{ margin: 0, color: '#f1f5f9' }}>Health Overview</h4>
            <button style={{ ...styles.btn, fontSize: 12 }} onClick={handleBatchHealth}>Run Batch Check</button>
          </div>
          <div style={{ display: 'flex', gap: 16, marginTop: 8 }}>
            {Object.entries(healthSum.counts || {}).map(([status, count]) => (
              <div key={status} style={{ textAlign: 'center' }}>
                <div style={{ fontSize: 24, fontWeight: 700, color: HEALTH_COLORS[status] || '#6b7280' }}>{count}</div>
                <div style={{ fontSize: 11, color: '#94a3b8' }}>{status}</div>
              </div>
            ))}
          </div>
          {healthSum.apps_with_issues?.length > 0 && (
            <div style={{ marginTop: 8, fontSize: 12, color: '#f87171' }}>
              Issues: {healthSum.apps_with_issues.map((a) => a.name).join(', ')}
            </div>
          )}
        </div>
      )}

      {/* Pending Apps */}
      <div style={styles.card}>
        <h4 style={{ margin: '0 0 8px', color: '#f1f5f9' }}>Pending Apps ({pending.length})</h4>
        {pending.length === 0 && <p style={{ color: '#64748b', fontSize: 13 }}>No pending submissions.</p>}
        {pending.map((app) => (
          <div key={app.id} style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: '8px 0', borderBottom: '1px solid #1e293b' }}>
            <div>
              <span style={{ color: '#f1f5f9', fontWeight: 500 }}>{app.name}</span>
              <span style={{ marginLeft: 8 }}><Badge label={app.protocol} color={PROTOCOL_COLORS[app.protocol] || '#6b7280'} small /></span>
              <div style={{ fontSize: 12, color: '#94a3b8' }}>{app.description?.slice(0, 80)}...</div>
            </div>
            <div style={{ display: 'flex', gap: 4 }}>
              <button style={{ ...styles.btn, background: '#166534', color: '#86efac', fontSize: 12 }} onClick={() => handleApprove(app.id)}>✓ Approve</button>
              <button style={{ ...styles.btn, background: '#7f1d1d', color: '#fca5a5', fontSize: 12 }} onClick={() => handleReject(app.id)}>✗ Reject</button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function TrendingPanel() {
  const [trending, setTrending] = useState([]);
  const [days, setDays] = useState(7);

  useEffect(() => {
    api.trendingApps(days, 10).then((r) => setTrending(r.data?.apps || [])).catch(() => {});
  }, [days]);

  return (
    <div style={styles.card}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
        <h4 style={{ margin: 0, color: '#f1f5f9' }}>🔥 Trending</h4>
        <select style={{ ...styles.input, width: 'auto', padding: '2px 8px', fontSize: 12 }} value={days} onChange={(e) => setDays(+e.target.value)}>
          <option value={1}>24h</option>
          <option value={7}>7 days</option>
          <option value={30}>30 days</option>
        </select>
      </div>
      {trending.length === 0 && <p style={{ color: '#64748b', fontSize: 13 }}>No trending apps yet.</p>}
      {trending.map((t, i) => (
        <div key={t.app_id} style={{ display: 'flex', gap: 8, alignItems: 'center', padding: '4px 0' }}>
          <span style={{ color: '#64748b', fontSize: 12, width: 18, textAlign: 'right' }}>#{i + 1}</span>
          <span style={{ color: '#f1f5f9', flex: 1 }}>{t.name}</span>
          <span style={{ color: '#818cf8', fontSize: 12 }}>{t.view_count} views</span>
        </div>
      ))}
    </div>
  );
}

export default function App() {
  const [hasKey, setHasKey] = useState(!!api.getKey());
  const [rateLimit, setRateLimit] = useState(null);
  const [apps, setApps] = useState([]);
  const [categories, setCategories] = useState([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [filterCategory, setFilterCategory] = useState('');
  const [filterProtocol, setFilterProtocol] = useState('');
  const [tab, setTab] = useState('browse'); // browse | submit | trending | admin
  const [selectedApp, setSelectedApp] = useState(null);
  const [loading, setLoading] = useState(false);

  const loadApps = useCallback(async () => {
    setLoading(true);
    try {
      let res;
      if (searchQuery.trim()) {
        let params = '';
        if (filterCategory) params += `&category=${filterCategory}`;
        if (filterProtocol) params += `&protocol=${filterProtocol}`;
        res = await api.searchApps(searchQuery, params);
        setApps(res.data?.apps || []);
      } else {
        let params = [];
        if (filterCategory) params.push(`category=${filterCategory}`);
        if (filterProtocol) params.push(`protocol=${filterProtocol}`);
        params.push('sort=newest');
        res = await api.listApps(params.join('&'));
        setApps(res.data?.apps || []);
      }
      if (res.rateLimit?.limit) setRateLimit(res.rateLimit);
    } catch (e) {
      console.error('Failed to load apps:', e);
    }
    setLoading(false);
  }, [searchQuery, filterCategory, filterProtocol]);

  const loadCategories = useCallback(async () => {
    try {
      const res = await api.listCategories();
      setCategories(res.data?.categories || []);
    } catch (e) { /* ignore */ }
  }, []);

  useEffect(() => {
    if (hasKey) {
      loadApps();
      loadCategories();
    }
  }, [hasKey, loadApps, loadCategories]);

  const handleSaveKey = (key) => {
    api.setKey(key);
    setHasKey(true);
  };

  const openApp = async (app) => {
    try {
      const res = await api.getApp(app.id);
      setSelectedApp(res.data);
    } catch {
      setSelectedApp(app);
    }
  };

  if (!hasKey) return <ApiKeyPrompt onSave={handleSaveKey} />;

  return (
    <div style={styles.container}>
      {/* Header */}
      <header style={styles.header}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          <h1 style={{ margin: 0, fontSize: 20, color: '#f1f5f9' }}>📂 App Directory</h1>
          <span style={{ fontSize: 12, color: '#64748b' }}>AI-First Application Discovery</span>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          {rateLimit && (
            <span style={{ fontSize: 11, color: '#64748b' }}>
              {rateLimit.remaining}/{rateLimit.limit} req
            </span>
          )}
          <button
            style={{ ...styles.btnGhost, fontSize: 11 }}
            onClick={() => { api.setKey(''); setHasKey(false); }}
          >
            🔑 Change Key
          </button>
        </div>
      </header>

      {/* Tabs */}
      <nav style={styles.tabs}>
        {[
          ['browse', '🔍 Browse'],
          ['submit', '➕ Submit'],
          ['trending', '🔥 Trending'],
          ['admin', '🛡️ Admin'],
        ].map(([id, label]) => (
          <button
            key={id}
            style={{ ...styles.tab, ...(tab === id ? styles.tabActive : {}) }}
            onClick={() => { setTab(id); setSelectedApp(null); }}
          >
            {label}
          </button>
        ))}
      </nav>

      {/* Content */}
      <main style={styles.main}>
        {tab === 'browse' && !selectedApp && (
          <>
            {/* Search & Filter Bar */}
            <div style={{ display: 'flex', gap: 8, marginBottom: 16, flexWrap: 'wrap' }}>
              <input
                style={{ ...styles.input, flex: 1, minWidth: 200 }}
                placeholder="Search apps..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && loadApps()}
              />
              <select style={{ ...styles.input, width: 'auto' }} value={filterCategory} onChange={(e) => setFilterCategory(e.target.value)}>
                <option value="">All Categories</option>
                {categories.map((c) => (
                  <option key={c.name} value={c.name}>{c.name} ({c.count})</option>
                ))}
              </select>
              <select style={{ ...styles.input, width: 'auto' }} value={filterProtocol} onChange={(e) => setFilterProtocol(e.target.value)}>
                <option value="">All Protocols</option>
                {PROTOCOLS.map((p) => <option key={p} value={p}>{p.toUpperCase()}</option>)}
              </select>
              <button style={{ ...styles.btn, ...styles.btnPrimary }} onClick={loadApps}>Search</button>
            </div>

            {loading ? (
              <p style={{ color: '#94a3b8', textAlign: 'center' }}>Loading...</p>
            ) : apps.length === 0 ? (
              <p style={{ color: '#64748b', textAlign: 'center' }}>No apps found. Try a different search or submit the first one!</p>
            ) : (
              <div style={styles.appGrid}>
                {apps.map((app) => (
                  <AppCard key={app.id} app={app} onClick={() => openApp(app)} />
                ))}
              </div>
            )}
          </>
        )}

        {tab === 'browse' && selectedApp && (
          <AppDetail
            app={selectedApp}
            onBack={() => setSelectedApp(null)}
            onRefresh={loadApps}
          />
        )}

        {tab === 'submit' && (
          <SubmitAppForm
            categories={categories}
            onSubmit={() => { setTab('browse'); loadApps(); }}
            onCancel={() => setTab('browse')}
          />
        )}

        {tab === 'trending' && <TrendingPanel />}

        {tab === 'admin' && <AdminPanel onRefresh={loadApps} />}
      </main>
    </div>
  );
}

const styles = {
  container: {
    minHeight: '100vh',
    background: '#0f172a',
    color: '#e2e8f0',
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
  },
  header: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    padding: '12px 20px',
    borderBottom: '1px solid #1e293b',
    background: '#0f172a',
    position: 'sticky',
    top: 0,
    zIndex: 10,
  },
  tabs: {
    display: 'flex',
    gap: 0,
    borderBottom: '1px solid #1e293b',
    background: '#0f172a',
    padding: '0 20px',
    position: 'sticky',
    top: 49,
    zIndex: 10,
  },
  tab: {
    padding: '10px 16px',
    background: 'none',
    border: 'none',
    borderBottom: '2px solid transparent',
    color: '#94a3b8',
    cursor: 'pointer',
    fontSize: 13,
    fontWeight: 500,
    transition: 'color 0.15s, border-color 0.15s',
  },
  tabActive: {
    color: '#818cf8',
    borderBottomColor: '#818cf8',
  },
  main: {
    maxWidth: 960,
    margin: '0 auto',
    padding: 20,
  },
  card: {
    background: '#1e293b',
    borderRadius: 8,
    padding: 16,
    border: '1px solid #334155',
    cursor: 'default',
    marginBottom: 8,
    transition: 'border-color 0.15s',
  },
  appGrid: {
    display: 'grid',
    gap: 8,
  },
  input: {
    background: '#0f172a',
    border: '1px solid #334155',
    borderRadius: 6,
    padding: '8px 12px',
    color: '#f1f5f9',
    fontSize: 13,
    outline: 'none',
    width: '100%',
    boxSizing: 'border-box',
  },
  label: {
    display: 'block',
    fontSize: 12,
    color: '#94a3b8',
    marginBottom: 4,
    fontWeight: 500,
  },
  formGrid: {
    display: 'grid',
    gridTemplateColumns: '1fr 1fr',
    gap: 12,
  },
  btn: {
    padding: '8px 16px',
    borderRadius: 6,
    border: 'none',
    cursor: 'pointer',
    fontSize: 13,
    fontWeight: 500,
    background: '#334155',
    color: '#e2e8f0',
  },
  btnPrimary: {
    background: '#4f46e5',
    color: '#fff',
  },
  btnGhost: {
    background: 'none',
    border: 'none',
    color: '#818cf8',
    cursor: 'pointer',
    fontSize: 13,
    padding: '4px 8px',
  },
  link: {
    color: '#818cf8',
    fontSize: 13,
    textDecoration: 'none',
  },
  overlay: {
    position: 'fixed',
    inset: 0,
    background: 'rgba(0,0,0,0.7)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    zIndex: 100,
  },
  modal: {
    background: '#1e293b',
    borderRadius: 12,
    padding: 24,
    width: 360,
    border: '1px solid #334155',
  },
};
