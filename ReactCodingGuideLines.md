# Component signatures
Define each component as a named `function` (not `const` with `FC`) that takes `props` as a single parameter. On the first line of the function body, destructure `props` with default values.

Plain JS:
```tsx
function UserCard(props) {
  const { name, age = 18, isAdmin = false } = props;
  return (
    <div>
      <h2>{name}</h2>
      <p>Age: {age}</p>
      {isAdmin && <strong>Admin</strong>}
    </div>
  );
}
```

TypeScript — define a `Props` type, pass it as the generic parameter on `props`:
```tsx
type TodoItemProps = {
  todo: Todo;
  onToggle: (id: number) => void;
};

export function TodoItem(props: TodoItemProps) {
  const { todo, onToggle } = props;

  return (
    <li>
      <input
        type="checkbox"
        checked={todo.done}
        onChange={() => onToggle(todo.id)}
      />
      <span style={{ textDecoration: todo.done ? "line-through" : "none" }}>
        {todo.text}
      </span>
    </li>
  );
}
```

> **Why not `FC`?** `React.FC` implicitly includes `children` in props (React 17) and obscures the return type. A plain function is simpler, easier to read, and what the React team recommends.

# UI testability
Make the controls ready for UI testing.
- Always add `data-testid` attributes for controls that show data (labels, text areas, etc.) or are interactive (buttons, links, selects, etc.).
- Add `role` attribute when the underlying HTML element or component library does not already provide the correct semantic role. Many MUI components supply roles automatically (e.g. `<Dialog>` → `role="dialog"`, `<Button>` → `role="button"`). Only add an explicit `role` when the rendered element would otherwise lack one (e.g. a `<div>` acting as a button, or a `<Typography>` acting as a heading).
- Add aria attributes where applicable, e.g. `aria-modal="true"`, `aria-labelledby="..."`, `aria-label="Search query"`, `aria-expanded="true"`, `aria-pressed="true"`.

# UI assertions
Test on values but also on visibility. Use `@testing-library/react` with Vitest:
```tsx
import { render, screen } from "@testing-library/react";
import { expect } from "vitest";

// Assert element is in the DOM and visible
expect(screen.getByTestId("add-todo")).toBeVisible();

// Assert text content
expect(screen.getByTestId("item-count")).toHaveTextContent("5");

// Assert element is NOT present
expect(screen.queryByTestId("error-message")).not.toBeInTheDocument();
```

# Test framework
Use **Vitest** + **@testing-library/react** + **@testing-library/user-event** for component tests.

## Installation
```
npm install -D vitest jsdom @testing-library/react @testing-library/user-event @testing-library/jest-dom
```

## Vite config (`vite.config.ts`)
```ts
export default defineConfig({
  // ...
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: "./src/__tests__/setup.ts",
    css: false,
  },
});
```

## Setup file (`src/__tests__/setup.ts`)
```ts
import "@testing-library/jest-dom/vitest";
```

## Test file pattern
- Place tests in `src/__tests__/<ComponentName>.test.tsx`
- Mock API hooks at the module level with `vi.mock()`
- Wrap rendered components in required providers (QueryClientProvider, BrowserRouter, etc.)
- Test: data renders correctly, visibility, loading state, empty state, interactions

Example:
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

vi.mock("../api/hooks", () => ({
  useItems: () => ({ data: [{ id: "1", name: "Item 1" }] }),
}));

describe("ItemsPage", () => {
  it("renders items", () => {
    render(<ItemsPage />);
    expect(screen.getByTestId("item-1")).toBeVisible();
    expect(screen.getByTestId("item-1")).toHaveTextContent("Item 1");
  });

  it("filters by search", async () => {
    render(<ItemsPage />);
    await userEvent.type(screen.getByTestId("search"), "xyz");
    expect(screen.queryByTestId("item-1")).not.toBeInTheDocument();
  });
});
```

# API access
## Installation
```
npm install @tanstack/react-query
```
## Setup
```tsx
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";

const queryClient = new QueryClient();

export default function Root() {
  return (
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  );
}
```

## Fetching Data with useQuery
```tsx
import { useQuery } from "@tanstack/react-query";
import { apiFetch } from "./api";

function Todos() {
  const { data, isLoading, error } = useQuery({
    queryKey: ["todos"],
    queryFn: () => apiFetch("/api/todos"),
  });

  if (isLoading) return <p>Loading...</p>;
  if (error) return <p>Error: {(error as Error).message}</p>;

  return (
    <ul>
      {data.map((todo) => (
        <li key={todo.id}>{todo.title}</li>
      ))}
    </ul>
  );
}
```
- `["todos"]` is the query key (used for caching).
- If another component also calls `useQuery({ queryKey: ["todos"] })`, React Query reuses the cache — no second API call!

## Mutations (POST/PUT/DELETE)
For non-GET requests (creating, updating, deleting), use useMutation.
```tsx
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { apiFetch } from "./api";

function AddTodo() {
  const queryClient = useQueryClient();

  const mutation = useMutation({
    mutationFn: (newTodo: { title: string }) =>
      apiFetch("/api/todos", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(newTodo),
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["todos"] });
    },
  });

  return (
    <button onClick={() => mutation.mutate({ title: "Learn React Query" })}>
      Add Todo
    </button>
  );
}
```

## Background Refresh & Stale Data

React Query automatically:
- Marks data as stale after a configurable time.
- Refetches in the background when the component mounts or window refocuses.

This keeps your UI up to date without manual refetching.

## Why this is powerful

- You never manually write "loading" or "error handling" boilerplate — it's built-in.
- You get token refresh in one place (the apiFetch helper).
- Components stay small and declarative.

# API Helper with Token Handling (reference)

> The sections below are reference material for projects that require authentication. Skip if your API is unauthenticated.

## Where to Store Tokens?

- Access Token (short-lived, e.g. 5-15 min)
Store in memory (or localStorage if you must).

    - Pros: safest in memory (not exposed to XSS if careful).
    - Cons: disappears on page reload — you need refresh token to get new one.

- Refresh Token (long-lived, e.g. days/weeks)
Best practice: keep it in an HTTP-only cookie set by your backend.

    - Browser stores it automatically
    - Not accessible to JavaScript — safer from XSS.
    - Sent automatically with requests to your auth endpoint.

If you can't do that (e.g. limited backend control), you might store it in localStorage or sessionStorage, but that's less secure.

## Token Utility with Active Expiry Check

Here's how you can actively refresh before the access token expires:

```ts
// auth.ts
let accessToken: string | null = null;
let refreshToken: string | null = null;
let expiryTime: number | null = null;

export function setTokens({ access, refresh, expiresIn }: {
  access: string; refresh: string; expiresIn: number;
}) {
  accessToken = access;
  refreshToken = refresh;
  expiryTime = Date.now() + expiresIn * 1000;
}

export function getAccessToken() {
  return accessToken;
}

async function refreshAccessToken() {
  const response = await fetch("/auth/refresh", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ refreshToken }),
  });

  if (!response.ok) {
    throw new Error("Refresh token invalid");
  }

  const data = await response.json();
  setTokens({
    access: data.accessToken,
    refresh: data.refreshToken ?? refreshToken,
    expiresIn: data.expiresIn,
  });
  return accessToken;
}

async function ensureValidToken() {
  if (!accessToken || !expiryTime || Date.now() > expiryTime - 30_000) {
    return await refreshAccessToken();
  }
  return accessToken;
}

export async function apiFetch(url: string, options: RequestInit = {}) {
  const token = await ensureValidToken();

  const headers = {
    ...options.headers,
    Authorization: `Bearer ${token}`,
  };

  const response = await fetch(url, { ...options, headers });

  if (!response.ok) {
    throw new Error("API error");
  }
  return response.json();
}
```

## Login Flow

On login, the backend should:
- Send back access token in JSON.
- Set refresh token cookie (HTTP-only, Secure, SameSite=strict/lax).

Example backend response headers (server-controlled):
```
Set-Cookie: refreshToken=abc123; HttpOnly; Secure; SameSite=Strict; Path=/auth/refresh
```

Client side on login:
```ts
async function login(username: string, password: string) {
  const response = await fetch("/auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username, password }),
    credentials: "include",
  });

  if (!response.ok) throw new Error("Login failed");

  const data = await response.json();
  setTokens(data);
}
```

## App Startup

When the app loads, you may not have an access token in memory (e.g., after page refresh).
You can immediately try to refresh using the refresh cookie:
```ts
async function initAuth() {
  try {
    await refreshAccessToken();
  } catch {
    // not logged in
  }
}
```

## Summary

- Access token: only in memory (not stored in browser storage).
- Refresh token: secure, HTTP-only cookie (set by backend, never touched by JS).
- Expiry check: decode JWT's exp claim, refresh proactively.
- App startup: immediately refresh with cookie to bootstrap access token.
- All requests: go through apiFetch which handles refresh automatically.
