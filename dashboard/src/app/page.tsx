"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";
import { getToken } from "@/lib/api";

export default function RootPage() {
  const router = useRouter();
  useEffect(() => {
    if (getToken()) {
      router.replace("/overview");
    } else {
      router.replace("/login");
    }
  }, [router]);
  return null;
}
