type WsTicketResponse = {
  access_token?: string;
  expires_in?: number;
};

interface OIDCBrokerClientOptions {
  authBaseUrl: string;
  bootstrapOidcToken?: string;
  refreshSkewMs?: number;
}

function normalizeAuthBaseUrl(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) {
    return '/auth';
  }
  if (/^https?:\/\//i.test(trimmed)) {
    return trimmed.replace(/\/+$/, '');
  }
  if (trimmed.startsWith('/')) {
    return trimmed.replace(/\/+$/, '');
  }
  return `/${trimmed.replace(/\/+$/, '')}`;
}

export class OIDCBrokerClient {
  private readonly authBaseUrl: string;
  private readonly refreshSkewMs: number;
  private bootstrapOidcToken?: string;
  private bootstrapped = false;
  private bootstrapping: Promise<boolean> | null = null;
  private cachedToken?: string;
  private cachedTokenExpiresAtMs = 0;
  private inFlightTicketPromise: Promise<string | undefined> | null = null;

  constructor(options: OIDCBrokerClientOptions) {
    this.authBaseUrl = normalizeAuthBaseUrl(options.authBaseUrl);
    this.bootstrapOidcToken = options.bootstrapOidcToken?.trim() || undefined;
    this.refreshSkewMs = Math.max(5_000, options.refreshSkewMs ?? 15_000);
  }

  setBootstrapOidcToken(token: string | undefined): void {
    this.bootstrapOidcToken = token?.trim() || undefined;
    if (this.bootstrapOidcToken) {
      this.bootstrapped = false;
    }
  }

  clearCache(): void {
    this.cachedToken = undefined;
    this.cachedTokenExpiresAtMs = 0;
  }

  async getWsAccessToken(): Promise<string | undefined> {
    const now = Date.now();
    if (this.cachedToken && this.cachedTokenExpiresAtMs - now > this.refreshSkewMs) {
      return this.cachedToken;
    }

    if (this.inFlightTicketPromise) {
      return this.inFlightTicketPromise;
    }

    this.inFlightTicketPromise = this.fetchWsTicket();
    try {
      return await this.inFlightTicketPromise;
    } finally {
      this.inFlightTicketPromise = null;
    }
  }

  private async fetchWsTicket(): Promise<string | undefined> {
    if (!this.bootstrapped && this.bootstrapOidcToken) {
      await this.ensureBrokerSession();
    }

    try {
      const response = await fetch(`${this.authBaseUrl}/ws-ticket`, {
        method: 'POST',
        credentials: 'include',
        headers: {
          'content-type': 'application/json',
        },
      });
      if (!response.ok) {
        return undefined;
      }

      const payload = (await response.json()) as WsTicketResponse;
      const token = payload.access_token?.trim();
      const expiresInSeconds = Number(payload.expires_in ?? 0);
      if (!token || !Number.isFinite(expiresInSeconds) || expiresInSeconds <= 0) {
        return undefined;
      }

      this.cachedToken = token;
      this.cachedTokenExpiresAtMs = Date.now() + expiresInSeconds * 1000;
      return token;
    } catch {
      return undefined;
    }
  }

  private async ensureBrokerSession(): Promise<boolean> {
    if (this.bootstrapped) {
      return true;
    }
    if (!this.bootstrapOidcToken) {
      return false;
    }
    if (this.bootstrapping) {
      return this.bootstrapping;
    }

    this.bootstrapping = (async () => {
      try {
        const response = await fetch(`${this.authBaseUrl}/oidc/exchange`, {
          method: 'POST',
          credentials: 'include',
          headers: {
            authorization: `Bearer ${this.bootstrapOidcToken}`,
            'content-type': 'application/json',
          },
          body: JSON.stringify({ id_token: this.bootstrapOidcToken }),
        });
        this.bootstrapped = response.ok;
        return response.ok;
      } catch {
        this.bootstrapped = false;
        return false;
      } finally {
        this.bootstrapping = null;
      }
    })();

    return this.bootstrapping;
  }
}
