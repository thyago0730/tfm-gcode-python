# Revisão técnica dos cálculos — TFM G-Code Generator

Data: 07/11/2026. Base: commit `f47cc25`. Escopo: todas as contas que o app usa para gerar o
G-code e para mostrar tempo, consumo, custo e qualidade.

## Método

1. Leitura das fórmulas no motor (`crates/tfm-core`): estimativas, modelo de sobreposição,
   modelo de depósito, qualidade, fórmulas editáveis do config e geradores de G-code.
2. Comparação com a própria saída do programa: o tempo "de máquina" foi somado linha a linha
   do G-code (distância lida ÷ F em cada `G01`, mais as esperas `G04`), e o volume depositado
   veio do modelo de depósito.
3. Comparação com referências técnicas externas, listadas no fim.

Todos os números abaixo usam os parâmetros padrão: Ø50 × 50 mm, cordão de 8 mm, 40% de
sobreposição, 1,5 mm por camada, 1 camada, 120 mm/min, 180 A, 25 V, gás a 15 L/min,
taxa de deposição de 700 g/h.

## Resumo

| # | Cálculo | Gravidade | Situação |
|---|---|---|---|
| C1 | Tempo das três oscilações | Crítica | 6 a 16 vezes maior que o real |
| C2 | Consumo de pó (e custo do pó) | Crítica | Valor fixo para todas as operações, sem base física |
| C3 | F com o eixo A no Mach3 | Crítica | Depende de uma configuração da máquina que o app não controla |
| A1 | Eficiência térmica do aporte | Alta | 0,8 no padrão; plasma é 0,6 |
| A2 | Diluição estimada | Alta | Heurística sem base; dá 36,6% onde o PTA dá 6 a 10% |
| A3 | Coerência entre taxa de deposição e espessura | Alta | Não é conferida; o padrão é incoerente |
| M1 | Tempo da Espiral | Média | 19% a mais |
| M2 | Tempo da Bolacha | Média | 10% a menos |
| M3 | Camadas inteligentes e compensação no tempo | Média | Ignoradas nas estimativas |
| M4 | Sobreposição padrão de 40% | Média | Acima do ótimo da literatura; precisa de calibração |
| M5 | Largura efetiva do cordão | Média | Correção de curvatura na direção errada e fator empírico |
| M6 | "Cobertura" e "Uniformidade" | Média | Rótulo trocado e nota sem base |
| B1 | Teto de 20 rpm fixo na Bolacha | Baixa | Ignora o perfil da máquina |
| B2 | Consumo de gás | Baixa | Só o tempo de arco |
| B3 | Custo de mão de obra e máquina | Baixa | Só o tempo de arco |

O que está certo, conferido abaixo: a forma da fórmula do aporte térmico, a rotação da Espiral,
o tempo da Chapa Plana e do Multiperfil, o volume do modelo de depósito, o apagamento da tocha
e a rotação da placa ao vivo.

---

## Críticas

### C1. Tempo das oscilações

**Para que serve.** Tempo de solda, que alimenta o custo de mão de obra, máquina e gás.

**Como é calculado.** Um único modelo para as três oscilações: por passo angular, uma ida e
volta em X de `oscilacao_comprimento` na velocidade de solda, mais o giro do passo; o número de
passos axiais é `comprimento ÷ (oscilação × sobreposição)`.

**Comparação com o G-code gerado:**

| Operação | Estimado | Máquina (G-code) | Passo mostrado | Passo do G-code |
|---|---|---|---|---|
| Oscilação Linear | 238,97 min | 14,45 min | 2,000 mm | 4,8 mm |
| Oscilação Quadrada | 238,97 min | 39,31 min | 2,000 mm | — |
| Quadrada Contínua | 206,25 min | 39,31 min | 2,000 mm | — |

**Divergências.**
- A Oscilação Linear não oscila em X: o G-code faz uma volta inteira em A por passo axial,
  com passo `largura × (1 − sobreposição)`. A estimativa usa o modelo da quadrada, e o "Passo
  Axial" mostrado (2,0 mm) não é o do programa (4,8 mm).
- Nas quadradas, as pernas em X andam no G-code na velocidade da oscilação (3000 mm/min), mas
  a estimativa usa a de solda (120 mm/min): 25 vezes mais lenta.

**Solução.** Tirar o tempo do próprio G-code gerado: soma de distância lida ÷ F em cada linha,
mais as esperas `G04` e os tempos de ignição. É a única fonte que não diverge do programa,
vale para todas as operações e já foi implementada neste estudo como verificação. Mostrar o
passo real do programa no painel.

### C2. Consumo de pó

**Para que serve.** Massa de pó por peça, custo do pó e planejamento de compra.

**Como é calculado hoje.** A fórmula editável do config `powder_mass_kg` sobrescreve o cálculo
de cada operação:

```
(π·D × (comprimento + leads) ÷ 10000) × (camadas × espessura) × powder_factor
```

**Divergências.**
- Dá **0,188 kg em todas as operações**, porque usa a área lateral de um cilindro até na
  Bolacha (disco) e na Chapa Plana (retângulo).
- A divisão por 10 000 leva mm² a dm², e o `powder_factor` padrão (0,16) não tem unidade
  definida. Na prática, equivale a supor 16 g/cm³ de pó gasto por cm³ depositado.
- A densidade do material escolhido (Stellite 6: 8,46 g/cm³; WC-Ni: 14,5 g/cm³) existe na
  tabela de materiais, mas não entra na conta.
- Não há rendimento de deposição (pó que vira cordão ÷ pó alimentado).

**Conta física, para comparar:**

| Item | Valor |
|---|---|
| Volume depositado (modelo de depósito, Espiral) | 11,8 cm³ |
| Massa depositada a 8,46 g/cm³ | 100 g |
| Pó com rendimento de 87% (literatura para PTA) | 115 g |
| Estimativa atual | 188 g |

**Solução.**
`massa de pó = volume depositado × densidade do material ÷ rendimento de deposição`, com:
- volume pela geometria de cada operação (ou pelo modelo de depósito, que já calcula);
- densidade do material escolhido;
- rendimento configurável por perfil de máquina e pó, com padrão de 0,85 a 0,87 para PTA,
  a calibrar pesando pó antes e depois de uma peça.

### C3. F com o eixo A no Mach3

**Para que serve.** É o F do G-code: define a velocidade real da tocha sobre a peça.

**Como é gerado.** O app escreve o F supondo que, num `G01` que move X e A juntos, o Mach3
trata os graus como milímetros (por exemplo, a Espiral sai com F ≈ graus por minuto).

**Divergência.** O manual do Mach3 (seção 10.1.6) diz que isso depende da configuração. Sem
o "raio do eixo A", o F é lido como graus por minuto. Com o raio configurado, o F vira mm por
minuto ao longo do caminho combinado. Se a máquina tiver o raio configurado, toda velocidade
de solda do programa sai errada por um fator perto de `raio × π ÷ 180`.

**Solução, em ordem de robustez:**
1. Gerar em **G93 (tempo inverso)**: cada linha traz o tempo dela (`F = 1 ÷ minutos`), e o
   resultado não depende da configuração do Mach3. É o padrão para eixos rotativos.
2. Enquanto isso, documentar a configuração exigida e validar na máquina: cronometrar uma
   volta da Espiral padrão, que deve durar 1,31 min.

---

## Altas

### A1. Eficiência térmica do aporte

**Para que serve.** Aporte térmico (kJ/mm), comparado às faixas da EPS e aos limites de
distorção e falta de fusão.

**Fórmula do app:** `aporte = η × U × I × 60 ÷ (1000 × v)`. A forma está certa (EN 1011-1 /
TWI).

**Divergências.**
- O padrão de η é 0,8. A EN 1011-1 e o TWI dão **0,6 para arco plasma**. Com o padrão, o
  aporte sai 33% alto: 1,80 kJ/mm em vez de 1,35 kJ/mm.
- O aviso de aporte (`process_warnings`) usa 0,8 fixo e ignora o valor informado.

**Solução.** Padrão de 0,6 para PTA, e usar sempre o parâmetro. Indicar ao lado do valor se
ele é "energia do arco" (η = 1, critério ASME) ou "aporte térmico" (EN), porque as EPS usam
um ou outro.

### A2. Diluição estimada

**Fórmula do app:** `clamp(2,5 × I ÷ (pó em g/min) − 2, 0, 100)`, herdada do Python.

**Divergência.** Não tem base física nem bibliográfica, e não é adimensional. Com o padrão dá
**36,6%**, enquanto a literatura de PTA indica diluição de 6 a 10%.

**Solução.** A diluição é razão de áreas na seção transversal: área fundida do substrato ÷
(área fundida + área adicionada). Opções:
1. Parar de mostrar como porcentagem e chamar de "indicador relativo".
2. Calibrar com macrografias de corpos de prova da sua EPS, ajustando um modelo por
   corrente, velocidade e alimentação.
3. Registrar a diluição medida no lote, para comparar previsto com realizado.

### A3. Coerência entre taxa de deposição e espessura

**O problema.** O app recebe a espessura por camada como dado e, separadamente, a taxa de
deposição (g/h), mas nunca confere uma contra a outra. Pelo balanço de massa:

`espessura = taxa (g/min) × rendimento ÷ (densidade × velocidade × passo)`

Com o padrão: 11,67 g/min ÷ (0,00846 g/mm³ × 120 mm/min × 4,8 mm) = **2,39 mm** com rendimento
de 100%, ou **2,08 mm** com 87%. O app usa 1,5 mm para subir o Z.

**Solução.** Mostrar a "espessura pelo balanço de massa" ao lado da pedida e avisar quando
diferir mais que 15%. O Z errado muda o afastamento da tocha a cada camada.

---

## Médias

### M1. Tempo da Espiral, 19% a mais

Estimado 16,22 min contra 13,64 min de máquina. A estimativa soma sempre dois anéis (720°),
mesmo com "Fechar início/fim" desligado. Solução: somar os anéis só quando existem, ou usar o
tempo do G-code (C1).

### M2. Tempo da Bolacha, 10% a menos

Estimado 3,82 min contra 4,22 min de máquina. A espiral usa o raio médio. Solução: tempo do
G-code (C1).

### M3. Camadas inteligentes e compensação

A camada raiz com velocidade reduzida (`% Vel. Raiz`) e a compensação de rotação mudam o F
do G-code, mas não entram no tempo nem no custo. O tempo do G-code (C1) resolve os dois.

### M4. Sobreposição padrão de 40%

O modelo de sobreposição tangente de Xiong e colegas (Robotics and CIM, 2014) mostra que a
distância entre centros ótima para superfície plana é cerca de **0,738 da largura**, ou 26%
de sobreposição. Com 40% (0,6 da largura) sobra material entre cordões, a superfície tende a
subir em degrau e o sobremetal cresce.

O modelo de depósito do app soma paraboloides que conservam volume. Ele não representa o
cordão escorrendo sobre o anterior, então mais sobreposição sempre parece mais lisa nele.

Solução: rodar corpos de prova a 26%, 33% e 40%, medir o perfil e ajustar o padrão. Ajustar
também o perfil do cordão no modelo (parábola, arco ou cosseno) pelas medidas.

### M5. Largura efetiva do cordão

- A correção de curvatura (`2R·arcsen(w ÷ 2R)`) é aplicada à largura do cordão. Na Espiral e
  nos anéis o cordão corre na circunferência, e a largura fica na direção axial, que é reta,
  sem curvatura. O efeito é pequeno, 0,4% em Ø50, mas o conceito está errado e cresce em
  diâmetros menores.
- O "fator de processo" (0,82 a 1,04 pela razão altura ÷ largura) é empírico e sem
  referência. Deve vir de calibração (`Calibração da largura`), não de uma curva fixa.

### M6. "Cobertura Estimada" e "Uniformidade"

O painel mostra "Cobertura Estimada: 40,0%", mas esse número é a sobreposição. A nota de
uniformidade usa pesos arbitrários (3,5; 140; 35). Solução: renomear, e trocar a nota pelos
números do modelo de depósito (sobremetal, mínimo e máximo), que já existem.

---

## Baixas

- **B1.** A Bolacha limita a rotação a 20 rpm fixo no gerador, enquanto o perfil da máquina
  tem 100 rpm. Usar o limite do perfil.
- **B2.** O gás conta só o tempo de arco: faltam pré e pós-fluxo, ignição e deslocamentos.
  Somar os tempos de `G04` e um pré e pós-fluxo configuráveis.
- **B3.** Mão de obra e máquina contam só o tempo de arco. Para orçamento, somar setup,
  preaquecimento e espera de interpasse, configuráveis por perfil.

---

## O que foi conferido e está certo

| Cálculo | Verificação |
|---|---|
| Fórmula do aporte térmico | Mesma forma da EN 1011-1 / TWI; só a eficiência padrão diverge (A1) |
| Rotação da Espiral (min/volta) | `comprimento da hélice ÷ velocidade`; bate com a rotação ao vivo em 2% |
| Tempo da Chapa Plana | 9,55 min estimado contra 9,55 min de máquina |
| Tempo do Multiperfil | 43,76 min contra 44,05 min (0,7%) |
| Volume do modelo de depósito | 11,8 cm³, igual a área × espessura pedida |
| Apagamento da tocha | Distância = velocidade × tempo de apagamento |
| Rotação da placa ao vivo | `F × |ΔA| ÷ (360 × distância lida)`; bate com o painel |

## Ordem sugerida de correção

1. **C1 + M1 + M2 + M3**: tempo pelo G-code gerado. Uma mudança só corrige quatro itens e
   destrava custo e gás confiáveis.
2. **C2**: pó por volume × densidade ÷ rendimento.
3. **A1**: eficiência 0,6 e uso do parâmetro no aviso.
4. **C3**: validar a configuração do Mach3 na máquina e, em seguida, gerar em G93.
5. **A3**: espessura pelo balanço de massa, com aviso.
6. **A2, M4, M5, M6**: dependem de corpos de prova medidos. Até lá, renomear o que é
   indicador e não medida.

## Referências

- TWI, "What is the difference between heat input and arc energy?" — fator de eficiência 0,6
  para arco plasma. https://www.twi-global.com/technical-knowledge/faqs/faq-what-is-the-difference-between-heat-input-and-arc-energy
- EN 1011-1 (fatores k), resumida em calculadoras como https://www.therness.com/tools/heat-input-calculator/
- IntechOpen, "Hardfacing by Plasma Transferred Arc Process" — diluição de 6 a 10% e rendimento
  de deposição perto de 87%. https://cdn.intechopen.com/pdfs/25058/InTech-Hardfacing_by_plasma_transferred_arc_process.pdf
- Xiong, Zhang, Gao, Wu, "A multi-bead overlapping model for robotic wire and arc additive
  manufacturing", Robotics and Computer-Integrated Manufacturing, 2014 — distância crítica de
  0,738 da largura. https://www.researchgate.net/publication/265689332
- Fórum oficial do Mach3, "How to set feedrate for mixed linear and rotary axes?", com o
  trecho das seções 10.1.6 e 10.7.25 do manual (F com eixos rotativos e G93).
  https://www.machsupport.com/forum/index.php?topic=15032.0
