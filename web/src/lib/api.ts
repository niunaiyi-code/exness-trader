import axios, { AxiosError } from 'axios';

export const API_BASE = import.meta.env.VITE_API_BASE || '/api/v1';

/** 会话存储键统一管理，避免散落的裸 localStorage 调用 */
const SESSION_KEYS = ['token', 'user_id', 'username', 'role'] as const;

export interface Session {
  token: string;
  userId: string;
  username: string;
  role: string;
}

export function getSession(): Session | null {
  const token = localStorage.getItem('token');
  if (!token) return null;
  return {
    token,
    userId: localStorage.getItem('user_id') || '',
    username: localStorage.getItem('username') || '',
    role: localStorage.getItem('role') || 'trader',
  };
}

export function saveSession(s: { token: string; user_id: string; username: string; role: string }): void {
  localStorage.setItem('token', s.token);
  localStorage.setItem('user_id', s.user_id);
  localStorage.setItem('username', s.username);
  localStorage.setItem('role', s.role);
}

export function clearSession(): void {
  for (const key of SESSION_KEYS) localStorage.removeItem(key);
}

/** 全局唯一 axios 实例：自动附 JWT，401 统一登出 */
export const api = axios.create({
  baseURL: API_BASE,
  timeout: 15000,
});

api.interceptors.request.use((config) => {
  const token = localStorage.getItem('token');
  if (token) config.headers.Authorization = `Bearer ${token}`;
  return config;
});

api.interceptors.response.use(
  (resp) => resp,
  (error: AxiosError) => {
    // 登录接口本身的 401（密码错误）不清会话，避免误刷新页面
    const isLoginRequest = error.config?.url?.includes('/auth/login');
    if (error.response?.status === 401 && !isLoginRequest) {
      clearSession();
      window.location.reload();
    }
    return Promise.reject(error);
  },
);

/** 从 axios 错误中提取后端 error 文案，用于 message.error 展示 */
export function apiErrorMessage(error: unknown, fallback: string): string {
  if (axios.isAxiosError(error)) {
    const data = error.response?.data as { error?: string } | undefined;
    return data?.error || error.message || fallback;
  }
  return fallback;
}
