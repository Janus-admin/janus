"use client";

import { useState, useEffect } from "react";
import { useRouter } from "next/navigation";
import { Button } from "@/components/ui/button";
import { auth } from "@/lib/api";
import { Key, Terminal, List, Bell, X, ChevronRight, ChevronLeft } from "lucide-react";

const STEPS = [
  {
    icon: Key,
    title: "Create your first API key",
    description:
      "API keys let your apps talk to Janus. Go to API Keys → New Key, give it a name and an optional budget limit.",
    action: { label: "Go to API Keys", href: "/keys" },
  },
  {
    icon: Terminal,
    title: "Send a test request",
    description:
      "Open the Playground and fire a chat completion. Change base_url to http://localhost:8080/v1 in any OpenAI SDK — that's the only change needed.",
    action: { label: "Open Playground", href: "/playground" },
  },
  {
    icon: List,
    title: "View your first request",
    description:
      "Every proxied call is logged. Head to Requests to see latency, token counts, cost, and cache status in real time.",
    action: { label: "Go to Requests", href: "/requests" },
  },
  {
    icon: Bell,
    title: "Set your first budget alert",
    description:
      "Create an alert to get notified when spend crosses a threshold. Janus supports Slack webhooks and email out of the box.",
    action: { label: "Go to Alerts", href: "/alerts" },
  },
];

interface OnboardingTourProps {
  onDismiss: () => void;
}

export function OnboardingTour({ onDismiss }: OnboardingTourProps) {
  const router = useRouter();
  const [step, setStep] = useState(0);
  const current = STEPS[step];
  const Icon = current.icon;
  const isLast = step === STEPS.length - 1;

  async function dismiss() {
    await auth.tourComplete().catch(() => {});
    onDismiss();
  }

  function navigate(href: string) {
    router.push(href);
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
      <div className="relative w-full max-w-md rounded-xl border bg-background shadow-2xl p-6 mx-4">
        {/* Dismiss */}
        <button
          onClick={dismiss}
          className="absolute top-4 right-4 text-muted-foreground hover:text-foreground transition-colors"
          aria-label="Skip tour"
        >
          <X className="h-4 w-4" />
        </button>

        {/* Step indicator */}
        <div className="flex gap-1.5 mb-6">
          {STEPS.map((_, i) => (
            <div
              key={i}
              className={`h-1 flex-1 rounded-full transition-colors ${
                i <= step ? "bg-primary" : "bg-muted"
              }`}
            />
          ))}
        </div>

        {/* Icon */}
        <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-primary/10">
          <Icon className="h-6 w-6 text-primary" />
        </div>

        {/* Content */}
        <h2 className="text-lg font-semibold mb-2">{current.title}</h2>
        <p className="text-sm text-muted-foreground mb-6 leading-relaxed">
          {current.description}
        </p>

        {/* Actions */}
        <div className="flex items-center gap-2">
          {step > 0 && (
            <Button variant="ghost" size="sm" onClick={() => setStep(step - 1)}>
              <ChevronLeft className="h-4 w-4 mr-1" />
              Back
            </Button>
          )}
          <div className="flex-1" />
          <Button
            variant="outline"
            size="sm"
            onClick={() => navigate(current.action.href)}
          >
            {current.action.label}
          </Button>
          {isLast ? (
            <Button size="sm" onClick={dismiss}>
              Done
            </Button>
          ) : (
            <Button size="sm" onClick={() => setStep(step + 1)}>
              Next
              <ChevronRight className="h-4 w-4 ml-1" />
            </Button>
          )}
        </div>

        {/* Skip link */}
        <p className="mt-4 text-center text-xs text-muted-foreground">
          Step {step + 1} of {STEPS.length} —{" "}
          <button
            onClick={dismiss}
            className="underline underline-offset-2 hover:text-foreground"
          >
            skip tour
          </button>
        </p>
      </div>
    </div>
  );
}

export function useOnboardingTour() {
  const [show, setShow] = useState(false);
  const [checked, setChecked] = useState(false);

  useEffect(() => {
    if (checked) return;
    setChecked(true);
    auth
      .me()
      .then((user) => {
        if (!user.tour_completed_at) setShow(true);
      })
      .catch(() => {});
  }, [checked]);

  return {
    show,
    dismiss: () => setShow(false),
  };
}
