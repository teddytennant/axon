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
            background: "#181818",
            border: "1px solid #222222",
            color: "#f5f5f5",
            fontFamily: "Space Grotesk, sans-serif",
          },
        }}
      />
    </QueryClientProvider>
  </StrictMode>,
);
