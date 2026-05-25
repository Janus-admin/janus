"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { cn } from "@/lib/utils";
import { useUIStore } from "@/store";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { useTheme } from "next-themes";
import { getToken, clearToken } from "@/lib/api";
import { useEffect, useState } from "react";
import { OnboardingTour, useOnboardingTour } from "@/components/OnboardingTour";
import {
  LayoutDashboard,
  List,
  Radio,
  BarChart3,
  Key,
  Cpu,
  Settings,
  Menu,
  X,
  Sun,
  Moon,
  Zap,
  LogOut,
  Bell,
  FileText,
  HeartPulse,
  Terminal,
  Calculator,
  Users,
} from "lucide-react";

const NAV = [
  { href: "/overview", label: "Overview", icon: LayoutDashboard },
  { href: "/requests", label: "Requests", icon: List },
  { href: "/live", label: "Live Feed", icon: Radio },
  { href: "/analytics", label: "Analytics", icon: BarChart3 },
  { href: "/analytics/simulate", label: "Cost Simulator", icon: Calculator },
  { href: "/keys", label: "API Keys", icon: Key },
  { href: "/providers", label: "Providers", icon: Cpu },
  { href: "/playground", label: "Playground", icon: Terminal },
  { href: "/health", label: "Health", icon: HeartPulse },
  { href: "/alerts", label: "Alerts", icon: Bell },
  { href: "/prompts", label: "Prompts", icon: FileText },
  { href: "/workspaces", label: "Workspaces", icon: Users },
  { href: "/settings", label: "Settings", icon: Settings },
];

function ThemeToggle() {
  const { theme, setTheme } = useTheme();
  return (
    <Button
      variant="ghost"
      size="icon"
      onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
      aria-label="Toggle theme"
    >
      <Sun className="h-4 w-4 rotate-0 scale-100 transition-all dark:-rotate-90 dark:scale-0" />
      <Moon className="absolute h-4 w-4 rotate-90 scale-0 transition-all dark:rotate-0 dark:scale-100" />
    </Button>
  );
}

function Sidebar({ open }: { open: boolean }) {
  const pathname = usePathname();
  const router = useRouter();

  function handleLogout() {
    clearToken();
    router.replace("/login");
  }

  return (
    <aside
      className={cn(
        "fixed inset-y-0 left-0 z-50 flex flex-col bg-sidebar border-r border-sidebar-border transition-all duration-200",
        open ? "w-56" : "w-14"
      )}
    >
      {/* Logo */}
      <div className="flex h-14 items-center gap-2 px-4 border-b border-sidebar-border">
        <Zap className="h-5 w-5 shrink-0 text-sidebar-primary" />
        {open && (
          <span className="font-semibold text-sidebar-foreground tracking-tight">
            Janus
          </span>
        )}
      </div>

      {/* Nav links */}
      <ScrollArea className="flex-1 py-3">
        <nav className="flex flex-col gap-0.5 px-2">
          {NAV.map(({ href, label, icon: Icon }) => {
            const active = pathname === href || pathname.startsWith(href + "/");
            return (
              <Link key={href} href={href}>
                <span
                  className={cn(
                    "flex items-center gap-3 rounded-md px-2 py-2 text-sm transition-colors cursor-pointer",
                    active
                      ? "bg-sidebar-primary text-sidebar-primary-foreground"
                      : "text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
                  )}
                >
                  <Icon className="h-4 w-4 shrink-0" />
                  {open && <span>{label}</span>}
                </span>
              </Link>
            );
          })}
        </nav>
      </ScrollArea>

      <Separator className="bg-sidebar-border" />
      <div className={cn("p-2 flex", open ? "justify-between" : "justify-center flex-col gap-1")}>
        <ThemeToggle />
        <Button
          variant="ghost"
          size="icon"
          onClick={handleLogout}
          aria-label="Sign out"
          className="text-muted-foreground hover:text-foreground"
        >
          <LogOut className="h-4 w-4" />
        </Button>
      </div>
    </aside>
  );
}

export default function DashboardLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const { sidebarOpen, toggleSidebar } = useUIStore();
  const router = useRouter();
  const [authed, setAuthed] = useState<boolean | null>(null);
  const { show: showTour, dismiss: dismissTour } = useOnboardingTour();

  useEffect(() => {
    if (getToken()) {
      setAuthed(true);
    } else {
      router.replace("/login");
    }
  }, [router]);

  // Show nothing while checking auth to prevent flash of dashboard.
  if (!authed) return null;

  return (
    <div className="flex h-full">
      {showTour && <OnboardingTour onDismiss={dismissTour} />}
      <Sidebar open={sidebarOpen} />

      <div
        className={cn(
          "flex flex-col flex-1 min-h-full transition-all duration-200",
          sidebarOpen ? "ml-56" : "ml-14"
        )}
      >
        <header className="sticky top-0 z-40 flex h-14 items-center gap-3 border-b bg-background/80 backdrop-blur px-4">
          <Button
            variant="ghost"
            size="icon"
            onClick={toggleSidebar}
            aria-label="Toggle sidebar"
          >
            {sidebarOpen ? (
              <X className="h-4 w-4" />
            ) : (
              <Menu className="h-4 w-4" />
            )}
          </Button>
          <span className="text-sm font-medium text-muted-foreground">
            Janus Admin
          </span>
        </header>

        <main className="flex-1 overflow-auto p-6">{children}</main>
      </div>
    </div>
  );
}
