import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Toaster } from "sonner";
import { App } from "./App";
import "./index.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 2,
      staleTime: 5000,
      refetchOnWindowFocus: false,
    },
  },
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
      <Toaster
        theme="dark"
        toastOptions={{
          style: {
            background: "#111111",
            border: "1px solid #1c1c1c",
            color: "#ffffff",
            fontFamily: "Inter, sans-serif",
            fontSize: "13px",
          },
        }}
      />
    </QueryClientProvider>
  </StrictMode>,
);
