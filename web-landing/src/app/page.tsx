import OrganicGrainyBackground from "@/components/OrganicGrainyBackground";

export default function Home() {
  return (
    <main className="min-h-screen w-full p-4 flex flex-col">
      <section className="relative w-full flex-1 min-h-[calc(100vh-2rem)] rounded-2xl overflow-hidden flex flex-col">
        <OrganicGrainyBackground
          className="w-full flex-1 min-h-[calc(100vh-2rem)] rounded-2xl p-4 flex flex-col"
          speed={0.12}
          animate={true}
          grainIntensity={0.075}
          waveIntensity={1.0}
        >
          {/* Page is kept clean for upcoming marketing design */}
        </OrganicGrainyBackground>
      </section>
    </main>
  );
}

