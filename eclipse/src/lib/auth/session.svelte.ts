import type {
  AdminExists,
  LoginRequest,
  LoginResponse,
  RegisterResponse,
  User
} from '$lib/api/generated';
import { ApiError, ApiTransport } from '$lib/api/transport';

export type SessionStatus = 'loading' | 'authenticated' | 'anonymous' | 'error';
const tokenKey = 'eclipse.session-token';

class SessionState {
  status = $state<SessionStatus>('loading');
  user = $state<User | null>(null);
  error = $state<string | null>(null);
  token = $state<string | null>(null);
  readonly api = new ApiTransport(() => this.token);

  async bootstrap() {
    this.status = 'loading';
    this.token = sessionStorage.getItem(tokenKey);
    try {
      this.user = await this.api.get<User>('auth/whoami');
      this.status = 'authenticated';
    } catch (error) {
      this.user = null;
      this.status =
        error instanceof ApiError && error.status === 401
          ? 'anonymous'
          : 'error';
      this.error = this.status === 'error' ? (error as Error).message : null;
      if (this.status === 'anonymous') this.clearToken();
    }
  }

  async login(credentials: LoginRequest) {
    const result = await this.api.post<LoginResponse>(
      'auth/login',
      credentials
    );
    await this.establish(result.token);
  }

  async adminExists() {
    const result = await this.api.get<AdminExists>('auth/admin_exists');
    return result.exists;
  }

  async register(credentials: LoginRequest) {
    const result = await this.api.post<RegisterResponse>(
      'auth/register',
      credentials
    );
    await this.establish(result.token);
  }

  private async establish(token: string) {
    this.token = token;
    sessionStorage.setItem(tokenKey, token);
    await this.bootstrap();
  }

  async logout() {
    try {
      await this.api.post<void>('auth/logout');
    } finally {
      this.clearToken();
      this.user = null;
      this.status = 'anonymous';
    }
  }

  expire() {
    this.clearToken();
    this.user = null;
    this.status = 'anonymous';
  }

  private clearToken() {
    this.token = null;
    sessionStorage.removeItem(tokenKey);
  }
}

export const session = new SessionState();
